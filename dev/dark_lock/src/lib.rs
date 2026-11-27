//! RNG Lands 装备锁定。
//!
//! 物品弹出菜单（背包里选中物品后的「使用 / 放置原地 / 丢弃 …」）末尾加一项「锁定」/「解锁」。
//!   菜单由游戏函数 POPUP 构建：先建好各选项（文字 + std::function 动作），再逐项 ADD_ITEM 进构建器，最后 FINALIZE 生成菜单。
//!   这里 hook POPUP（记下这次的物品信息，标记「正在建物品菜单」）和 FINALIZE（在物品菜单里追加我们的选项）。
//!   文字编号 4190 锁定 / 4191 解锁（GR_MenuText + GR_LineHelp，bossres.py 写进 menu_dlc02）。
//! 物品：POPUP 第 4 个参数（物品上下文）+0x4c 物品编号、+0x50 物品实例句柄（高 4 位 8 武器 / 9 防具 / A 护符 / B 道具）。
//!   只给武器、防具、护符加选项。句柄不固定（强化武器、读档后会换），所以锁定按（存档格子, 类别, 编号）记，
//!   武器编号去掉强化等级（末两位，和 dark_sell 的 sell_key 一样）：强化后仍是锁定的；同一种的几件一起锁定（和护符一样）。
//!   名单在 %APPDATA%\EldenRing\DarkReforged\角色记录\lock_items.txt（和配方、存档放一起，整合包换目录 / 更新不丢；
//!   2026-10-29 以前在 EldenRing 根目录的 DarkReforged_lock_items.txt，新位置没有时读旧的），
//!   一行「格子<TAB>类别（0 武器 / 1 防具 / 2 护符）<TAB>编号<TAB>角色名」（角色名只是方便看）；还没有这个文件时读一次
//!   旧版写在 DLL 旁边的 lock_items.txt（下次改动时写到新位置，旧文件不动）；旧版按句柄记的行（「格子<TAB>句柄<TAB>编号」，
//!   十六进制）读的时候换成新键，下次改动时按新格式写回。
//! 选一次选项游戏会调两次动作：同一件 0.5 秒内只算一次。
//! 联机时还加「送给 <队友>」（联机计划 13.7，2026-11-16）：能送给几个队友问 dark_core 的 dr_gift_can（0 = 不加），
//!   第 i 个用文字 4192 + i（sc/lock.py；选项文字是占位，dark_core 改写成队友的名字），选了调 dr_gift_begin（dark_core 的 gear/gift.rs）。
//! 格子左上的锁图标（背包、仓库）：
//!   - OPEN_FILE（游戏读界面文件）读到 02_020_inventory / 03_050_itembox.gfx 时，用 gfx::patch 往格子元件里加一个
//!     矢量锁 RngLock，把 MemoryFile 的数据指针换成改好的（磁盘上的文件不动，不管是原版还是别的 mod 的界面文件）；
//!   - BIND（格子绑定子元素）前用游戏的 ResolvePath 把 RngLock 解析进栈上的临时元素包装，原函数之后设成隐藏；
//!   - REFRESH（格子按数据刷新）后从格子自己的根元素包装（格子 +0，BIND 里游戏把根元素复制到这里）再解析一次 RngLock，
//!     按数据 +0x50 的物品实例句柄是否锁定设置可见。
//!   临时包装用完照游戏自己的做法析构（BIND 里同样的「解析 → 设可见 → 析构」）：包装里只有 +0x28 起的值对象（GFx::Value，持有元素的引用）
//!   要析构（VAL_DTOR）。以前每次 BIND 泄漏一个 0x60 字节的包装（还一直拿着元素的引用），格子地址也一直记着（崩溃风险审查 2026-11-19 第 26 条）。
//! 游戏脚本覆盖包这次没加载（启动器在 DLL 旁边放 no_menu_text.txt）：文字 4190~4196 不在，不加「锁定 / 送给」（第 17 条；锁图标照常）。
//! 武器补正格加宽（dark_core 把补正字母换成数字后三位数放不下，用户 2026-10-29）：同一个 OPEN_FILE 读到
//!   02_055_detailstatus_weapon1 / 03_030_comparison_equip_weapon1.gfx 时用 gfx::widen_scaling 把补正那几格加宽。
//! 认 eldenring.exe 2.7.1.0（1.17.1）和 2.7.2.0（1.17.2）：按各函数开头的字节认版本，都对不上就什么都不做，只在 dark_lock.log 里记一行。

mod gfx;

use std::cell::Cell;
use std::ffi::CStr;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use eldenring::cs::{EquipInventoryData, GameDataMan, GameMan};
use fromsoftware_shared::FromStatic;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualAlloc, VirtualProtect,
};

// ---- 游戏函数：每个支持的 exe 版本一组 RVA；按各函数开头的字节认版本
struct Rvas {
    version: &'static str,
    popup: usize,       // 建物品弹出菜单 (?, ?, 物品信息*, 物品上下文*, ?)
    finalize: usize,    // 菜单构建器 → 菜单 (构建器, 输出*, ?)
    add_item: usize,    // 构建器加一项 (构建器, 文字对*, std::function*, 0) -> 构建器
    text_label: usize,  // 文字对的选项文字 (out, GR_MenuText 编号)
    text_help: usize,   // 文字对的底部说明 (out, GR_LineHelp 编号)
    text_dtor: usize,   // 文字对析构
    result2: usize,     // 动作的返回值（引用计数对象，代码 2）
    call_fin: usize,    // POPUP 里调 FINALIZE 的那条 call
    open_file: usize,   // CSScaleformFileOpener::OpenFile (opener, 路径, flags, mode) -> MemoryFile*（地址出自 ERNativeUI）
    memfile_vt: usize,  // MemoryFile 虚表：+0x18 数据指针、+0x20 长度、+0x24 读取位置、+0x28 有效
    memfile_vt0: usize, // 虚表第 0 项（核对用）
    bind: usize,        // 物品格子绑定子元素 (格子, 根元素*)
    refresh: usize,     // 物品格子按数据刷新 (格子, 数据*)
    resolve: usize,     // 元素包装 ResolvePath (根元素*, 输出包装*, 名字) -> 输出包装*
    setvis: usize,      // 元素包装设可见 (包装*, bool)
    val_dtor: usize,    // 元素包装 +0x28 的值对象析构 (包装 + 0x28)：BIND 里临时包装用完就调它（bind + 0x53 那条 call，matches 里核对）
    /// FINALIZE 开头 16 字节（末尾是调 __chkstk 的 call，偏移随版本变；其他函数的开头各版本一样）
    finalize_head: [u8; 16],
}

static VERSIONS: [Rvas; 2] = [
    Rvas {
        version: "2.7.1.0（1.17.1）",
        popup: 0x8fc570,
        finalize: 0x78e780,
        add_item: 0x745600,
        text_label: 0x7617c0,
        text_help: 0x7615e0,
        text_dtor: 0x743ae0,
        result2: 0x7a80d0,
        call_fin: 0x8fd02d,
        open_file: 0xd6d280,
        memfile_vt: 0x2ba7d70,
        memfile_vt0: 0xce9730,
        bind: 0x900940,
        refresh: 0x900610,
        resolve: 0x74b140,
        setvis: 0x734190,
        val_dtor: 0xd81600,
        finalize_head: [0x40, 0x55, 0x56, 0x57, 0x41, 0x56, 0x41, 0x57, 0xb8, 0x10, 0x33, 0x00, 0x00, 0xe8, 0x8e, 0xf6],
    },
    // 1.17.2：界面代码整体后移 0x4B0，读文件 / 虚表那一段后移 0x16F0 / 0x22B0
    Rvas {
        version: "2.7.2.0（1.17.2）",
        popup: 0x8fca20,
        finalize: 0x78ec30,
        add_item: 0x745ab0,
        text_label: 0x761c70,
        text_help: 0x761a90,
        text_dtor: 0x743f90,
        result2: 0x7a8580,
        call_fin: 0x8fd4dd,
        open_file: 0xd6e970,
        memfile_vt: 0x2baa020,
        memfile_vt0: 0xceae20,
        bind: 0x900df0,
        refresh: 0x900ac0,
        resolve: 0x74b5f0,
        setvis: 0x734640,
        val_dtor: 0xd82cf0,
        finalize_head: [0x40, 0x55, 0x56, 0x57, 0x41, 0x56, 0x41, 0x57, 0xb8, 0x10, 0x33, 0x00, 0x00, 0xe8, 0x0e, 0x09],
    },
];

impl Rvas {
    /// 核对用的（RVA, 开头字节）
    fn checks(&self) -> [(usize, &[u8]); 14] {
        [
            (self.open_file, &[0x40, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d, 0x6c, 0x24, 0xd9]),
            (self.bind, &[0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50, 0x10, 0x55, 0x48, 0x8d, 0x68, 0xa1, 0x48, 0x81, 0xec, 0x90]),
            (self.refresh, &[0x40, 0x55, 0x56, 0x57, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d, 0x6c, 0x24, 0xc9, 0x48, 0x81, 0xec]),
            (self.resolve, &[0x4c, 0x89, 0x44, 0x24, 0x18, 0x4c, 0x89, 0x4c, 0x24, 0x20, 0x55, 0x53, 0x56, 0x57, 0x41, 0x56]),
            (self.setvis, &[0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x01, 0x0f, 0xb6, 0xda, 0xff, 0x50, 0x08, 0x8b]),
            (self.popup, &[0x40, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8d, 0xac]),
            (self.finalize, &self.finalize_head),
            (self.add_item, &[0x4c, 0x89, 0x4c, 0x24, 0x20, 0x55, 0x53, 0x56, 0x57, 0x48, 0x8d, 0xac, 0x24, 0x68, 0xfe, 0xff]),
            (self.text_label, &[0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x48, 0x83, 0xec, 0x40, 0x48, 0xc7, 0x44, 0x24, 0x38, 0xfe]),
            (self.text_help, &[0x4c, 0x8b, 0xdc, 0x57, 0x48, 0x81, 0xec, 0x90, 0x00, 0x00, 0x00, 0x49, 0xc7, 0x43, 0xa8, 0xfe]),
            (self.text_dtor, &[0x48, 0x89, 0x4c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x30, 0x48, 0xc7, 0x44, 0x24, 0x20, 0xfe]),
            (self.result2, &[0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x48, 0x83, 0xec, 0x30, 0x48, 0xc7, 0x44, 0x24, 0x28, 0xfe]),
            (self.call_fin, &[0xe8, 0x4e, 0x17, 0xe9, 0xff]), // call FINALIZE：两个版本的相对偏移一样
            // BIND 开头把根元素复制进格子 +0（mov rdi,rdx; mov rbx,rcx; call 复制）：REFRESH 时从格子 +0 解析 RngLock 靠的就是它
            (self.bind + 0x23, &[0x48, 0x8b, 0xfa, 0x48, 0x8b, 0xd9, 0xe8, 0xe2, 0xaf, 0xe4, 0xff]),
        ]
    }
}

/// 格子数据里物品实例句柄的位置
const DATA_HANDLE: usize = 0x50;
const TEXT_LOCK: u32 = 4190;
const TEXT_UNLOCK: u32 = 4191;
/// 「送给第 i 个队友」的文字 4192 + i，最多 5 个（和 dark_core 的 gear/gift.rs MAX_PEERS、sc/lock.py 一致）
const TEXT_GIFT0: u32 = 4192;
const MAX_GIFT: u32 = 5;
const MAX_POPUP_LOGS: u32 = 3;

type PopupFn = unsafe extern "C" fn(usize, usize, usize, usize, usize) -> usize;
type FinalizeFn = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
type AddItemFn = unsafe extern "C" fn(usize, *mut u8, *mut usize, usize) -> usize;
type TextFn = unsafe extern "C" fn(*mut u8, u32) -> usize;
type DtorFn = unsafe extern "C" fn(*mut u8);
type ResultFn = unsafe extern "C" fn(*mut usize) -> *mut usize;
type OpenFileFn = unsafe extern "C" fn(usize, *const u8, i32, i32) -> usize;
type CellFn = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
type ResolveFn = unsafe extern "C" fn(usize, *mut u8, *const u8, usize) -> usize;
type SetVisFn = unsafe extern "C" fn(*mut u8, u8);
type ValDtorFn = unsafe extern "C" fn(*mut u8);

static DIR: OnceLock<PathBuf> = OnceLock::new();
static BASE: AtomicUsize = AtomicUsize::new(0);
/// 认出来的版本（install 里设，挂钩之前）
static RV: OnceLock<&'static Rvas> = OnceLock::new();
static ORIG_POPUP: AtomicUsize = AtomicUsize::new(0);
static ORIG_FIN: AtomicUsize = AtomicUsize::new(0);
static POPUP_LOGS: AtomicUsize = AtomicUsize::new(0);
static ORIG_OPEN: AtomicUsize = AtomicUsize::new(0);
static ORIG_BIND: AtomicUsize = AtomicUsize::new(0);
static ORIG_REFRESH: AtomicUsize = AtomicUsize::new(0);
/// 经过 BIND 的格子（REFRESH 只给这些解析 RngLock：格子 +0 的根元素包装是 BIND 里填的）。只记地址，格子换了同一个地址就是同一项
static CELL_SEEN: Mutex<Option<std::collections::HashSet<usize>>> = Mutex::new(None);
/// 这次游戏脚本覆盖包没加载（DLL 旁边有 no_menu_text.txt，启动器写的）：「锁定 / 送给」的文字不在，不加这两种选项
static NO_MENU_TEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const NO_MENU_TEXT_FILE: &str = "no_menu_text.txt";
/// 改过的界面文件：（长度, FNV 哈希）→ 改好的字节（MovieDef 会一直用这个指针，所以永不释放）；None = 不用改
/// 界面文件的改写函数（gfx::patch 加锁图标 / gfx::widen_scaling 加宽补正格）
type GfxPatch = fn(&[u8]) -> Result<Option<Vec<u8>>, String>;

static GFX_CACHE: Mutex<Option<HashMap<(usize, u64), Option<&'static [u8]>>>> = Mutex::new(None);
/// 锁定键：（存档格子, 类别 0 武器 / 1 防具 / 2 护符, 编号（武器去掉强化等级））
type Key = (i32, u8, u32);
/// 这次物品菜单的物品：（键, 物品编号）；不是装备 / 没进存档就是 None
static CURRENT: Mutex<Option<(Key, u32)>> = Mutex::new(None);
/// 锁定名单：键 → 角色名
static LOCKED: Mutex<Option<HashMap<Key, String>>> = Mutex::new(None);
static LAST_TOGGLE: Mutex<Option<(Key, std::time::Instant)>> = Mutex::new(None);
/// 句柄 → 物品编号（当前角色的背包 + 仓库），和建表的时间
static ITEM_IDS: Mutex<Option<(HashMap<u32, u32>, std::time::Instant)>> = Mutex::new(None);
/// 物品上下文里编号、句柄的位置
const CTX_ID: usize = 0x4c;
const CTX_HANDLE: usize = 0x50;

thread_local! {
    static IN_POPUP: Cell<u32> = const { Cell::new(0) };
}

fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."))
}

fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir().join("dark_lock.log")) {
        let t = local_time();
        let _ = writeln!(f, "{:02}:{:02}:{:02}.{:03} {msg}", t.hour, t.min, t.sec, t.ms);
    }
}

/// 本地时间：日志每行开头写时:分:秒.毫秒，加载时写一行日期（直接调 kernel32，不用给 windows crate 加特性）
#[repr(C)]
#[derive(Default)]
struct LocalTime {
    year: u16,
    month: u16,
    _weekday: u16,
    day: u16,
    hour: u16,
    min: u16,
    sec: u16,
    ms: u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLocalTime(t: *mut LocalTime);
}

fn local_time() -> LocalTime {
    let mut t = LocalTime::default();
    unsafe { GetLocalTime(&mut t) };
    t
}

fn module_dir(h: u64) -> PathBuf {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(HMODULE(h as *mut _)), &mut buf) } as usize;
    let p = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn rv() -> &'static Rvas {
    RV.get().copied().unwrap_or(&VERSIONS[0])
}

fn addr(rva: usize) -> usize {
    BASE.load(Ordering::Relaxed) + rva
}

// ---- 锁定名单（%APPDATA%\EldenRing\DarkReforged\角色记录\lock_items.txt，一行一个键；没有 APPDATA 就放 DLL 旁边）
/// 联机（Seamless Co-op 加载了）时放 角色记录\联机\（见 ersc_loaded）
fn lock_file() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|a| {
            let d = PathBuf::from(a).join("EldenRing").join("DarkReforged").join("角色记录");
            if ersc_loaded() { d.join("联机") } else { d }.join("lock_items.txt")
        })
        .unwrap_or_else(old_lock_file)
}

/// Seamless Co-op（ersc.dll）加载了没有：加载了游戏就只用联机存档（ER0000.rng.co2，格子编号和单机各算各的），
/// 角色记录要放 角色记录\联机\（dev\coop\联机计划.md 1.1，dark_core 的 coop.rs 同样判断）。查到一次就记住；没加载时每次现查
fn ersc_loaded() -> bool {
    static ERSC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ERSC.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let on = unsafe { GetModuleHandleW(windows::core::w!("ersc.dll")) }.is_ok();
    if on {
        ERSC.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    on
}

/// 2026-10-29 以前的名单位置：%APPDATA%\EldenRing 根目录（启动器会搬进 角色记录\，这里兜底）
fn root_lock_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("EldenRing").join("DarkReforged_lock_items.txt"))
}

/// 旧版（0.5.2 以前）的名单位置：DLL 旁边
fn old_lock_file() -> PathBuf {
    dir().join("lock_items.txt")
}

/// 句柄的类别：8 武器 / 9 防具 / A 护符 → 0 / 1 / 2，其他（道具等）None
fn cat_of(handle: u32) -> Option<u8> {
    match handle >> 28 {
        0x8 => Some(0),
        0x9 => Some(1),
        0xA => Some(2),
        _ => None,
    }
}

/// 锁定用的编号：去掉类别位；武器再去掉强化等级（末两位），强化前后是同一个键
fn key_id(cat: u8, id: u32) -> u32 {
    let pid = id & 0x0FFF_FFFF;
    if cat == 0 { pid - pid % 100 } else { pid }
}

/// 名单的一行 →（键, 角色名）：新格式「格子 类别 编号(十进制) 角色名」；旧格式「格子 句柄 编号 角色名」（句柄 8 位十六进制）
fn parse_line(line: &str) -> Option<(Key, String)> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 3 {
        return None;
    }
    let slot = f[0].parse::<i32>().ok()?;
    let name = f.get(3).unwrap_or(&"").to_string();
    let (cat, id) = if f[1].len() == 8 {
        (cat_of(u32::from_str_radix(f[1], 16).ok()?)?, u32::from_str_radix(f[2], 16).ok()?)
    } else {
        (f[1].parse::<u8>().ok().filter(|c| *c <= 2)?, f[2].parse::<u32>().ok()?)
    };
    Some(((slot, cat, key_id(cat, id)), name))
}

fn with_locked<R>(f: impl FnOnce(&mut HashMap<Key, String>) -> R) -> R {
    let mut g = LOCKED.lock().unwrap();
    let map = g.get_or_insert_with(|| {
        // 联机时不退回旧位置（那些是单机角色的）
        let text = fs::read_to_string(lock_file())
            .or_else(|e| if ersc_loaded() { Err(e) } else { root_lock_file().map_or(Err(e), fs::read_to_string) })
            .or_else(|e| if ersc_loaded() { Err(e) } else { fs::read_to_string(old_lock_file()) })
            .unwrap_or_default();
        text.lines().filter_map(parse_line).collect()
    });
    prune_fresh(map);
    f(map)
}

// ---- dark_core 的角色身份（who.rs）：认准了一个刚建的角色（游戏时间不到 3 分钟）就清掉那个格子以前的锁定
// （用户 2026-11-19「收拢」：锁定名单只按格子记，新角色会接着用这个格子以前那个角色的锁定）。dark_core 也会改文件，
// 这里清内存里的、再存一次，免得下次存名单时把旧的写回去
type WhoFreshFn = unsafe extern "C" fn(*mut i32) -> u32;
static WHO_FRESH: AtomicUsize = AtomicUsize::new(0);
static FRESH_SEEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn who_fresh() -> Option<(u32, i32)> {
    if WHO_FRESH.load(Ordering::Relaxed) == 0 {
        let name: Vec<u16> = "dark_core.dll\0".encode_utf16().collect();
        let m = unsafe { GetModuleHandleW(windows::core::PCWSTR(name.as_ptr())) }.ok()?;
        let f = unsafe { windows::Win32::System::LibraryLoader::GetProcAddress(m, windows::core::s!("dr_who_fresh")) }?;
        WHO_FRESH.store(f as usize, Ordering::Relaxed);
    }
    let f = unsafe { std::mem::transmute::<usize, WhoFreshFn>(WHO_FRESH.load(Ordering::Relaxed)) };
    let mut slot = -1;
    let seq = unsafe { f(&mut slot) };
    Some((seq, slot))
}

fn prune_fresh(map: &mut HashMap<Key, String>) {
    let Some((seq, slot)) = who_fresh() else { return };
    if seq == 0 || FRESH_SEEN.swap(seq, Ordering::Relaxed) == seq || slot < 0 {
        return;
    }
    let before = map.len();
    map.retain(|k, _| k.0 != slot);
    if map.len() != before {
        save_locked(map);
        log(&format!("格子 {slot} 是刚建的角色：清掉这个格子以前的 {} 条锁定", before - map.len()));
    }
}

fn save_locked(map: &HashMap<Key, String>) {
    let mut v: Vec<_> = map.iter().collect();
    v.sort_by_key(|(k, _)| **k);
    let mut out = String::from("# 格子\t类别（0 武器 / 1 防具 / 2 护符）\t编号（武器不含强化等级）\t角色名\n");
    for ((slot, cat, id), name) in v {
        out += &format!("{slot}\t{cat}\t{id}\t{name}\n");
    }
    let p = lock_file();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    if fs::write(&p, out).is_err() {
        log(&format!("锁定名单写不进 {}", p.display()));
    }
}

/// 把这个格子的这把武器从弃置名单删掉（锁定即取消弃置，用户 2026-11-26），返回删了没有。
/// 弃置名单是 dark_core 的（装备工坊「超限」页武器栏第 4 个按钮写的），和 lock_items.txt 同一个目录
/// （联机时 角色记录\联机\）。两边都在游戏线程里跑，而且每次都现读现改、不缓存，不会真并发
fn drop_discard(slot: i32, base: u32) -> bool {
    let p = lock_file().with_file_name("discard_items.txt");
    let Ok(text) = fs::read_to_string(&p) else { return false };
    let mut out = String::from("# 格子\t类别（0 武器）\t编号（生成武器的 base = 参数行 id / 10000 * 10000）\t角色名\n");
    let mut hit = false;
    for l in text.lines().map(|l| l.trim_end_matches('\r')) {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() >= 3 && f[0].parse::<i32>().ok() == Some(slot) && f[2].parse::<u32>().ok() == Some(base) {
            hit = true;
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    if !hit {
        return false;
    }
    // 内存里那份弃置名单立刻作废，格子下次刷新就别再画红叉了（缓存 500 毫秒，玩家以为没生效）
    *DISCARDED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let tmp = p.with_extension("txt.tmp");
    if fs::write(&tmp, out).and_then(|_| fs::rename(&tmp, &p)).is_err() {
        log(&format!("弃置名单写不进 {}", p.display()));
        return false;
    }
    true
}

/// 当前角色背包 + 仓库里的 句柄 → 物品编号
fn scan_inventory() -> HashMap<u32, u32> {
    let mut m = HashMap::new();
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else { return m };
    let pgd = &gdm.main_player_game_data;
    let mut add = |inv: &EquipInventoryData| {
        for it in inv.items_data.items() {
            let h: u32 = unsafe { std::mem::transmute_copy(&it.gaitem_handle) };
            m.insert(h, it.item_id.into_inner());
        }
    };
    add(&pgd.equipment.equip_inventory_data);
    if let Some(st) = &pgd.storage {
        add(st);
    }
    m
}

/// 句柄 → 物品编号：表 1 秒内有效；查不到时重建，但 0.2 秒内不重复建（格子刷新很频繁）
fn item_id(handle: u32) -> Option<u32> {
    let mut g = ITEM_IDS.lock().unwrap();
    if let Some((m, t)) = g.as_ref() {
        let age = t.elapsed().as_millis();
        if let Some(id) = m.get(&handle).filter(|_| age < 1000) {
            return Some(*id);
        }
        if age < 200 {
            return None;
        }
    }
    let m = std::panic::catch_unwind(scan_inventory).ok()?;
    let r = m.get(&handle).copied();
    *g = Some((m, std::time::Instant::now()));
    r
}

/// 当前角色：（存档格子, 角色名）；没进存档（等级 0）或读不到就是 None
fn character() -> Option<(i32, String)> {
    std::panic::catch_unwind(|| {
        let gm = unsafe { GameMan::instance() }.ok()?;
        let gdm = unsafe { GameDataMan::instance() }.ok()?;
        let pgd = &gdm.main_player_game_data;
        if pgd.level == 0 || gm.save_slot < 0 {
            return None;
        }
        let n = pgd.character_name.iter().position(|&c| c == 0).unwrap_or(pgd.character_name.len());
        Some((gm.save_slot, String::from_utf16_lossy(&pgd.character_name[..n])))
    })
    .ok()
    .flatten()
}

// ---- dark_core 的赠送接口（找不到 = 不是联机分支的 dark_core，或者还没加载：不加选项，下次建菜单再找）
type GiftCanFn = unsafe extern "C" fn(u32, u32) -> u32;
type GiftBeginFn = unsafe extern "C" fn(u32, u32, u32);
static GIFT_CAN: AtomicUsize = AtomicUsize::new(0);
static GIFT_BEGIN: AtomicUsize = AtomicUsize::new(0);

fn gift_api() -> Option<(GiftCanFn, GiftBeginFn)> {
    if GIFT_CAN.load(Ordering::Relaxed) == 0 {
        let name: Vec<u16> = "dark_core.dll\0".encode_utf16().collect();
        let m = unsafe { GetModuleHandleW(windows::core::PCWSTR(name.as_ptr())) }.ok()?;
        let can = unsafe { windows::Win32::System::LibraryLoader::GetProcAddress(m, windows::core::s!("dr_gift_can")) }?;
        let begin = unsafe { windows::Win32::System::LibraryLoader::GetProcAddress(m, windows::core::s!("dr_gift_begin")) }?;
        GIFT_BEGIN.store(begin as usize, Ordering::Relaxed);
        GIFT_CAN.store(can as usize, Ordering::Relaxed);
    }
    let (c, b) = (GIFT_CAN.load(Ordering::Relaxed), GIFT_BEGIN.load(Ordering::Relaxed));
    (c != 0 && b != 0).then(|| unsafe { (std::mem::transmute::<usize, GiftCanFn>(c), std::mem::transmute::<usize, GiftBeginFn>(b)) })
}

/// 「送给第 i 个队友」的动作
#[repr(C)]
struct GiftFn {
    vtable: *const GiftVTable,
    cat: u32,
    id: u32,
    i: u32,
}

#[repr(C)]
struct GiftVTable([*const (); 6]);
unsafe impl Sync for GiftVTable {}

static GIFT_VTABLE: GiftVTable = GiftVTable([
    gift_copy as *const (),
    gift_copy as *const (),
    gift_call as *const (),
    fn_target as *const (),
    gift_delete as *const (),
    gift_get as *const (),
]);

static LAST_GIFT: Mutex<Option<((u32, u32, u32), std::time::Instant)>> = Mutex::new(None);

unsafe extern "C" fn gift_copy(this: *const GiftFn, _where: *mut u8) -> *mut GiftFn {
    let (cat, id, i) = unsafe { ((*this).cat, (*this).id, (*this).i) };
    Box::into_raw(Box::new(GiftFn { vtable: &GIFT_VTABLE, cat, id, i }))
}

unsafe extern "C" fn gift_delete(this: *mut GiftFn, dealloc: u8) {
    if dealloc != 0 {
        drop(unsafe { Box::from_raw(this) });
    }
}

unsafe extern "C" fn gift_get(this: *const GiftFn) -> *const GiftFn {
    this
}

/// 选中「送给 <队友>」：交给 dark_core（同一件同一个队友 0.5 秒内只算一次），返回游戏的结果对象
unsafe extern "C" fn gift_call(this: *const GiftFn, ret: *mut usize, _arg: usize) -> *mut usize {
    let k = unsafe { ((*this).cat, (*this).id, (*this).i) };
    let dup = {
        let mut last = LAST_GIFT.lock().unwrap();
        let dup = matches!(*last, Some((x, t)) if x == k && t.elapsed().as_millis() < 500);
        *last = Some((k, std::time::Instant::now()));
        dup
    };
    if !dup {
        if let Some((_, begin)) = gift_api() {
            unsafe { begin(k.0, k.1, k.2) };
            log(&format!("送给第 {} 个队友：类别 {} 物品编号 {:08x}（交给 dark_core）", k.2 + 1, k.0, k.1));
        }
    }
    let mut tmp = 0usize;
    unsafe {
        let mk: ResultFn = std::mem::transmute(addr(rv().result2));
        mk(&mut tmp);
        *ret = tmp;
    }
    ret
}

// ---- 我们的 std::function 实现（MSVC _Func_base：_Copy, _Move, _Do_call, _Target_type, _Delete_this, _Get）
#[repr(C)]
struct LockFn {
    vtable: *const VTable,
    key: Key,
    id: u32,
}

#[repr(C)]
struct VTable([*const (); 6]);
unsafe impl Sync for VTable {}

static VTABLE: VTable = VTable([
    fn_copy as *const (),
    fn_copy as *const (),
    fn_call as *const (),
    fn_target as *const (),
    fn_delete as *const (),
    fn_get as *const (),
]);

/// 总是在堆上复制一份（不放进 std::function 的内部缓冲区）：游戏那边看到指针不在缓冲区里，就会用 _Delete_this(true) 释放
unsafe extern "C" fn fn_copy(this: *const LockFn, _where: *mut u8) -> *mut LockFn {
    Box::into_raw(Box::new(LockFn { vtable: &VTABLE, key: unsafe { (*this).key }, id: unsafe { (*this).id } }))
}

unsafe extern "C" fn fn_target(_this: *const LockFn) -> usize {
    0
}

unsafe extern "C" fn fn_delete(this: *mut LockFn, dealloc: u8) {
    if dealloc != 0 {
        drop(unsafe { Box::from_raw(this) });
    }
}

unsafe extern "C" fn fn_get(this: *const LockFn) -> *const LockFn {
    this
}

/// 选中「锁定 / 解锁」：切换锁定，返回游戏的结果对象
unsafe extern "C" fn fn_call(this: *const LockFn, ret: *mut usize, arg: usize) -> *mut usize {
    let (key, id) = unsafe { ((*this).key, (*this).id) };
    let dup = {
        let mut last = LAST_TOGGLE.lock().unwrap();
        let dup = matches!(*last, Some((k, t)) if k == key && t.elapsed().as_millis() < 500);
        *last = Some((key, std::time::Instant::now()));
        dup
    };
    if !dup {
        let name = character().map(|c| c.1).unwrap_or_default();
        let now = with_locked(|m| {
            let now = m.remove(&key).is_none();
            if now {
                m.insert(key, name.clone());
            }
            save_locked(m);
            now
        });
        log(&format!("{}：格子 {} 类别 {} 编号 {}（物品编号 {id:08x}，{name}，参数 {arg:#x}）", if now { "锁定" } else { "解锁" }, key.0, key.1, key.2));
        // 用户 2026-11-26：刚锁定的武器如果标了弃置，顺手把弃置取消（锁定 = 保护、弃置 = 待处理，两者不能同时成立）
        if now && key.1 == 0 && drop_discard(key.0, key.2) {
            log(&format!("锁定：格子 {} 编号 {} 同时取消了弃置", key.0, key.2));
        }
    }
    let mut tmp = 0usize;
    unsafe {
        let mk: ResultFn = std::mem::transmute(addr(rv().result2));
        mk(&mut tmp);
        *ret = tmp;
    }
    ret
}

// ---- hook
unsafe extern "C" fn popup_hook(a: usize, b: usize, info: usize, ctx: usize, e: usize) -> usize {
    let mut key = None;
    if ctx != 0 {
        let (id, h) = unsafe { (*((ctx + CTX_ID) as *const u32), *((ctx + CTX_HANDLE) as *const u32)) };
        if let Some(cat) = cat_of(h) {
            key = character().map(|(slot, _)| ((slot, cat, key_id(cat, id)), id));
        }
    }
    if info != 0 {
        let raw = unsafe { std::slice::from_raw_parts(info as *const u8, 0x58) };
        if POPUP_LOGS.fetch_add(1, Ordering::Relaxed) < MAX_POPUP_LOGS as usize {
            let c = if ctx != 0 { hex(unsafe { std::slice::from_raw_parts(ctx as *const u8, 0x88) }) } else { "-".into() };
            log(&format!("物品菜单：a={a:#x} b={b:#x} e={e:#x}\n  信息 {}\n  上下文 {c}", hex(raw)));
        }
    }
    *CURRENT.lock().unwrap() = key;
    IN_POPUP.with(|c| c.set(c.get() + 1));
    let orig: PopupFn = unsafe { std::mem::transmute(ORIG_POPUP.load(Ordering::Relaxed)) };
    let r = unsafe { orig(a, b, info, ctx, e) };
    IN_POPUP.with(|c| c.set(c.get() - 1));
    r
}

unsafe extern "C" fn finalize_hook(builder: usize, out: usize, x: usize, y: usize) -> usize {
    if IN_POPUP.with(|c| c.get()) > 0 && !NO_MENU_TEXT.load(Ordering::Relaxed) {
        if let Some(key) = *CURRENT.lock().unwrap() {
            if let Err(e) = std::panic::catch_unwind(|| unsafe { add_lock_item(builder, key) }) {
                log(&format!("加选项出错：{e:?}"));
            }
            if let Err(e) = std::panic::catch_unwind(|| unsafe { add_gift_items(builder, key) }) {
                log(&format!("加赠送选项出错：{e:?}"));
            }
        }
    }
    let orig: FinalizeFn = unsafe { std::mem::transmute(ORIG_FIN.load(Ordering::Relaxed)) };
    unsafe { orig(builder, out, x, y) }
}

#[repr(C, align(16))]
struct TextPair([u8; 0x70]);

unsafe fn add_lock_item(builder: usize, (key, item): (Key, u32)) {
    let locked = with_locked(|m| m.contains_key(&key));
    let id = if locked { TEXT_UNLOCK } else { TEXT_LOCK };
    unsafe {
        let label: TextFn = std::mem::transmute(addr(rv().text_label));
        let help: TextFn = std::mem::transmute(addr(rv().text_help));
        let dtor: DtorFn = std::mem::transmute(addr(rv().text_dtor));
        let add: AddItemFn = std::mem::transmute(addr(rv().add_item));
        let mut pair = TextPair([0; 0x70]);
        label(pair.0.as_mut_ptr(), id);
        help(pair.0.as_mut_ptr().add(0x38), id);
        let mut func = [0usize; 8]; // std::function：实现指针在 +0x38
        func[7] = Box::into_raw(Box::new(LockFn { vtable: &VTABLE, key, id: item })) as usize;
        add(builder, pair.0.as_mut_ptr(), func.as_mut_ptr(), 0); // 会复制一份并释放 func 里的这份
        dtor(pair.0.as_mut_ptr());
    }
}

/// 联机时「送给 <队友>」：dark_core 说能送给几个就加几项
unsafe fn add_gift_items(builder: usize, ((_slot, cat, _kid), item): (Key, u32)) {
    let Some((can, _)) = gift_api() else { return };
    let n = unsafe { can(cat as u32, item) }.min(MAX_GIFT);
    for i in 0..n {
        unsafe {
            let label: TextFn = std::mem::transmute(addr(rv().text_label));
            let help: TextFn = std::mem::transmute(addr(rv().text_help));
            let dtor: DtorFn = std::mem::transmute(addr(rv().text_dtor));
            let add: AddItemFn = std::mem::transmute(addr(rv().add_item));
            let mut pair = TextPair([0; 0x70]);
            label(pair.0.as_mut_ptr(), TEXT_GIFT0 + i);
            help(pair.0.as_mut_ptr().add(0x38), TEXT_GIFT0 + i);
            let mut func = [0usize; 8]; // std::function：实现指针在 +0x38
            func[7] = Box::into_raw(Box::new(GiftFn { vtable: &GIFT_VTABLE, cat: cat as u32, id: item, i })) as usize;
            add(builder, pair.0.as_mut_ptr(), func.as_mut_ptr(), 0);
            dtor(pair.0.as_mut_ptr());
        }
    }
}

// ---- 格子上的锁图标
fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &c| (h ^ c as u64).wrapping_mul(0x100000001b3))
}

/// 把 MemoryFile 换成改好的界面文件（f：改写函数，没得改返回 None）
unsafe fn patch_memory_file(file: usize, f: GfxPatch) -> Result<String, String> {
    unsafe {
        if *(file as *const usize) != addr(rv().memfile_vt) {
            return Err("不是 MemoryFile".into());
        }
        let bytes = *((file + 0x18) as *const *const u8);
        let len = *((file + 0x20) as *const i32);
        let (cursor, valid) = (*((file + 0x24) as *const i32), *((file + 0x28) as *const u8));
        if bytes.is_null() || len < 21 || cursor != 0 || valid != 1 {
            return Err(format!("状态不对（长度 {len} 位置 {cursor} 有效 {valid}）"));
        }
        let src = std::slice::from_raw_parts(bytes, len as usize);
        let mut g = GFX_CACHE.lock().unwrap();
        let cache = g.get_or_insert_with(HashMap::new);
        let key = (src.len(), fnv(src));
        let out = match cache.get(&key) {
            Some(x) => *x,
            None => {
                let x = f(src)?.map(|v| &*Box::leak(v.into_boxed_slice()));
                cache.insert(key, x);
                x
            }
        };
        let Some(out) = out else { return Ok("没有要改的 / 已经改过，不改".into()) };
        *((file + 0x18) as *mut *const u8) = out.as_ptr();
        *((file + 0x20) as *mut i32) = out.len() as i32;
        *((file + 0x24) as *mut i32) = 0;
        Ok(format!("{len} → {} 字节", out.len()))
    }
}

unsafe extern "C" fn open_file_hook(opener: usize, path: *const u8, flags: i32, mode: i32) -> usize {
    let orig: OpenFileFn = unsafe { std::mem::transmute(ORIG_OPEN.load(Ordering::Relaxed)) };
    let file = unsafe { orig(opener, path, flags, mode) };
    if file != 0 && !path.is_null() {
        let p = unsafe { CStr::from_ptr(path as *const std::ffi::c_char) }.to_string_lossy().to_lowercase();
        let f: Option<GfxPatch> = if p.ends_with("02_020_inventory.gfx") || p.ends_with("03_050_itembox.gfx") {
            Some(gfx::patch)
        } else if p.ends_with("02_055_detailstatus_weapon1.gfx") || p.ends_with("03_030_comparison_equip_weapon1.gfx") {
            Some(gfx::widen_scaling)
        } else {
            None
        };
        if let Some(f) = f {
            match std::panic::catch_unwind(|| unsafe { patch_memory_file(file, f) }) {
                Ok(Ok(m)) => log(&format!("界面文件 {p}：{m}")),
                Ok(Err(e)) => log(&format!("界面文件 {p}：没改（{e}）")),
                Err(_) => log(&format!("界面文件 {p}：改写出错（没改）")),
            }
        }
    }
    file
}

#[repr(C, align(16))]
struct Elem([u8; 0x60]);

/// 在 root（元素包装）下解析一个按名字找的子元素，放进栈上的临时包装 e，返回包装指针
/// （ResolvePath 在输出包装上从头构造，不看原来的内容）
unsafe fn resolve_named(root: usize, e: &mut Elem, name: &str) -> *mut u8 {
    let w = e.0.as_mut_ptr();
    let n = [name.as_bytes(), b"\0"].concat();
    unsafe {
        let resolve: ResolveFn = std::mem::transmute(addr(rv().resolve));
        resolve(root, w, n.as_ptr(), 0);
    }
    w
}

/// 设可见（不析构包装）
unsafe fn set_vis(w: *mut u8, show: bool) {
    let setvis: SetVisFn = unsafe { std::mem::transmute(addr(rv().setvis)) };
    unsafe { setvis(w, show as u8) };
}

/// 照游戏的做法析构临时包装：只有 +0x28 的值对象要析构（放掉对元素的引用），前面的部分没有要释放的
unsafe fn drop_wrap(w: *mut u8) {
    let dtor: ValDtorFn = unsafe { std::mem::transmute(addr(rv().val_dtor)) };
    unsafe { dtor(w.add(0x28)) };
}

unsafe extern "C" fn bind_hook(cell: usize, root: usize, x: usize, y: usize) -> usize {
    // 根元素是按值传进来的临时对象，原函数结尾会析构它，所以要在调原函数之前解析；原函数之后设成隐藏、析构
    let mut e1 = Elem([0; 0x60]);
    let w1 = unsafe { resolve_named(root, &mut e1, gfx::LOCK_NAME) };
    let mut e2 = Elem([0; 0x60]);
    let w2 = unsafe { resolve_named(root, &mut e2, gfx::DISCARD_NAME) };
    let orig: CellFn = unsafe { std::mem::transmute(ORIG_BIND.load(Ordering::Relaxed)) };
    let r = unsafe { orig(cell, root, x, y) };
    unsafe {
        set_vis(w1, false);
        set_vis(w2, false);
        drop_wrap(w1);
        drop_wrap(w2);
    }
    if let Ok(mut g) = CELL_SEEN.lock() {
        g.get_or_insert_with(Default::default).insert(cell);
    }
    r
}

/// 格子上的物品锁没锁：句柄换成编号再查（这个角色一件都没锁就不用查背包）
fn is_locked(handle: u32) -> bool {
    let (Some(cat), Some((slot, _))) = (cat_of(handle), character()) else { return false };
    if !with_locked(|m| m.keys().any(|k| k.0 == slot)) {
        return false;
    }
    item_id(handle).is_some_and(|id| with_locked(|m| m.contains_key(&(slot, cat, key_id(cat, id)))))
}

/// 弃置名单（角色记录[\联机]\discard_items.txt）：(存档格子, base) 的集合。
/// 这个文件由 dark_core 写，这里只读；格子刷新每帧都在问，所以缓存 500 毫秒再重读一次（玩家点完弃置最多半秒就看到红叉）
static DISCARDED: Mutex<Option<(std::time::Instant, std::collections::HashSet<(i32, u32)>)>> = Mutex::new(None);

fn discard_file() -> PathBuf {
    lock_file().with_file_name("discard_items.txt")
}

fn with_discarded<R>(f: impl FnOnce(&std::collections::HashSet<(i32, u32)>) -> R) -> R {
    let mut g = DISCARDED.lock().unwrap_or_else(|e| e.into_inner());
    let stale = g.as_ref().map_or(true, |(t, _)| t.elapsed().as_millis() >= 500);
    if stale {
        let text = fs::read_to_string(discard_file()).unwrap_or_default();
        let set = text
            .lines()
            .map(|l| l.trim_end_matches('\r'))
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                Some((f.first()?.parse::<i32>().ok()?, f.get(2)?.parse::<u32>().ok()?))
            })
            .collect();
        *g = Some((std::time::Instant::now(), set));
    }
    f(&g.as_ref().unwrap().1)
}

/// 生成武器的 base：参数行 id / 10000 * 10000（每把独占一行，强化前后不变）
fn discard_base(id: u32) -> u32 {
    (id & 0x0FFF_FFFF) / 10000 * 10000
}

/// 格子上的物品弃置了没有（只武器）
fn is_discarded(handle: u32) -> bool {
    let (Some(cat), Some((slot, _))) = (cat_of(handle), character()) else { return false };
    if cat != 0 {
        return false;
    }
    let Some(id) = item_id(handle) else { return false };
    let base = discard_base(id);
    let hit = with_discarded(|m| m.contains(&(slot, base)));
    if hit {
        // 命中时记一行，方便反馈"红叉没出现"时区分「没命中」和「命中了没画出来」；格子刷新很频繁，最多 5 秒一行
        static AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        let mut g = AT.lock().unwrap_or_else(|e| e.into_inner());
        if g.map_or(true, |t| t.elapsed().as_secs() >= 5) {
            *g = Some(std::time::Instant::now());
            log(&format!("弃置红叉：格子 {slot} 的 {base} 该显示（名单 {} 条）", with_discarded(|m| m.len())));
        }
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_old_and_new() {
        // 旧格式：按句柄（十六进制）；武器编号去掉强化等级
        assert_eq!(parse_line("6\t80831123\t0bc04530\tD"), Some(((6, 0, 197150000), "D".into())));
        assert_eq!(parse_line("2\t90800010\t10123456\t"), Some(((2, 1, 0x00123456), "".into())));
        assert_eq!(parse_line("2\ta0001f40\t20001f40\tX"), Some(((2, 2, 0x1f40), "X".into())));
        assert_eq!(parse_line("2\tb0000001\t00000001\tX"), None); // 道具不算
        // 新格式：类别 + 十进制编号；强化过的武器编号也换成基础编号
        assert_eq!(parse_line("6\t0\t197150004\tD"), Some(((6, 0, 197150000), "D".into())));
        assert_eq!(parse_line("6\t2\t4150\tD"), Some(((6, 2, 4150), "D".into())));
        assert_eq!(parse_line("6\t3\t4150\tD"), None);
        assert_eq!(parse_line("# 格子\t类别\t编号\t角色名"), None);
        assert_eq!(key_id(0, 197150004), 197150000);
        assert_eq!(key_id(1, 0x10000000 | 1234), 1234);
    }
}

unsafe extern "C" fn refresh_hook(cell: usize, data: usize, x: usize, y: usize) -> usize {
    let orig: CellFn = unsafe { std::mem::transmute(ORIG_REFRESH.load(Ordering::Relaxed)) };
    let r = unsafe { orig(cell, data, x, y) };
    let seen = data != 0 && CELL_SEEN.lock().is_ok_and(|g| g.as_ref().is_some_and(|s| s.contains(&cell)));
    if seen {
        let h = unsafe { *((data + DATA_HANDLE) as *const u32) };
        let (lock, disc) = (is_locked(h), is_discarded(h));
        // 格子 +0 是它的根元素包装（BIND 开头游戏复制进来的，REFRESH 自己也在用）：从它再解析一次，用完析构，不留着。
        // 两个都要先解析、再设可见：set_vis_and_drop 末尾会析构包装里的值对象，先析构第一个可能影响第二次解析
        // （2026-11-27 实测：红叉结构插对了、判断也命中了，就是画不出来）
        let mut e1 = Elem([0; 0x60]);
        let mut e2 = Elem([0; 0x60]);
        unsafe {
            let w1 = resolve_named(cell, &mut e1, gfx::LOCK_NAME);
            let w2 = resolve_named(cell, &mut e2, gfx::DISCARD_NAME);
            // 两个都设完可见再一起析构：析构会放掉包装里对元素的引用，先析构第一个可能影响第二次设置
            set_vis(w1, lock);
            set_vis(w2, disc);
            drop_wrap(w1);
            drop_wrap(w2);
        }
    }
    r
}

/// 内联 hook：目标开头写 `mov rax, hook; jmp rax`（12 字节）+ nop 补到 stolen，跳板 = 原来的 stolen 字节 + 跳回。
/// stolen 必须正好是完整指令的边界、且这些指令里没有 rip 相对寻址（各 hook 点都核对过）
unsafe fn detour(target: usize, hook: usize, stolen: usize) -> Result<usize, String> {
    unsafe {
        let tramp = VirtualAlloc(None, 64, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as *mut u8;
        if tramp.is_null() {
            return Err("VirtualAlloc 失败".into());
        }
        std::ptr::copy_nonoverlapping(target as *const u8, tramp, stolen);
        let back = tramp.add(stolen);
        back.copy_from_nonoverlapping([0xFF, 0x25, 0, 0, 0, 0].as_ptr(), 6);
        (back.add(6) as *mut usize).write_unaligned(target + stolen);
        let mut old = PAGE_PROTECTION_FLAGS(0);
        VirtualProtect(target as *const _, stolen, PAGE_EXECUTE_READWRITE, &mut old).map_err(|e| format!("VirtualProtect：{e}"))?;
        let mut patch = vec![0x90u8; stolen];
        patch[0] = 0x48;
        patch[1] = 0xB8;
        patch[2..10].copy_from_slice(&hook.to_le_bytes());
        patch[10] = 0xFF;
        patch[11] = 0xE0;
        std::ptr::copy_nonoverlapping(patch.as_ptr(), target as *mut u8, stolen);
        let mut tmp = PAGE_PROTECTION_FLAGS(0);
        let _ = VirtualProtect(target as *const _, stolen, old, &mut tmp);
        Ok(tramp as usize)
    }
}

/// 这个版本的各函数开头字节、MemoryFile 虚表是否都对得上
fn matches(base: usize, v: &Rvas) -> Result<(), String> {
    for (rva, bytes) in v.checks() {
        let mem = unsafe { std::slice::from_raw_parts((base + rva) as *const u8, bytes.len()) };
        if mem != bytes {
            return Err(format!("exe+{rva:#x} 的字节对不上（{}）", hex(mem)));
        }
    }
    let vt0 = unsafe { *((base + v.memfile_vt) as *const usize) };
    if vt0 != base + v.memfile_vt0 {
        return Err(format!("MemoryFile 虚表对不上（{:#x}）", vt0.wrapping_sub(base)));
    }
    // BIND 里析构临时包装的那条：lea rcx,[rbp+17h]（临时包装 + 0x28）; call 值对象析构
    let at = base + v.bind + 0x4f;
    let b = unsafe { std::slice::from_raw_parts(at as *const u8, 9) };
    let tgt = (at + 9).wrapping_add(i32::from_le_bytes([b[5], b[6], b[7], b[8]]) as isize as usize);
    if b[..5] != [0x48, 0x8d, 0x4d, 0x17, 0xe8] || tgt != base + v.val_dtor {
        return Err(format!("BIND 里值对象析构的调用对不上（{}）", hex(b)));
    }
    Ok(())
}

fn install() -> Result<&'static Rvas, String> {
    let base = unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandle：{e}"))?.0 as usize;
    BASE.store(base, Ordering::Relaxed);
    let mut why = Vec::new();
    let Some(v) = VERSIONS.iter().find(|v| matches(base, v).map_err(|e| why.push(format!("按 {}：{e}", v.version))).is_ok()) else {
        return Err(format!("游戏版本不是 2.7.1.0 / 2.7.2.0（{}）", why.join("；")));
    };
    let _ = RV.set(v);
    unsafe {
        // 开头 13 字节：push×8 + mov / lea；绑定 12 字节：mov rax,rsp / mov [rax+10],rdx / push rbp / lea rbp,[rax-5f]；读文件 16 字节：push×7 + lea
        // （两个版本这几段字节一样）
        ORIG_FIN.store(detour(base + v.finalize, finalize_hook as *const () as usize, 13)?, Ordering::Relaxed);
        ORIG_POPUP.store(detour(base + v.popup, popup_hook as *const () as usize, 13)?, Ordering::Relaxed);
        ORIG_OPEN.store(detour(base + v.open_file, open_file_hook as *const () as usize, 16)?, Ordering::Relaxed);
        ORIG_BIND.store(detour(base + v.bind, bind_hook as *const () as usize, 12)?, Ordering::Relaxed);
        ORIG_REFRESH.store(detour(base + v.refresh, refresh_hook as *const () as usize, 13)?, Ordering::Relaxed);
    }
    Ok(v)
}

/// # Safety
/// 由加载器（YAFSML / me3）调用，不要自己调用。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(hmodule: u64, reason: u32, _reserved: *mut std::ffi::c_void) -> i32 {
    if reason != 1 {
        return 1;
    }
    let _ = DIR.set(module_dir(hmodule));
    std::panic::set_hook(Box::new(|info| log(&format!("panic：{info}"))));
    let t = local_time();
    log(&format!("==== {}-{:02}-{:02} 加载（进程 {}）", t.year, t.month, t.day, std::process::id()));
    match install() {
        Ok(v) => log(&format!(
            "dark_lock 已加载（eldenring.exe {}）：物品弹出菜单里加「锁定 / 解锁」，背包 / 仓库格子显示锁图标和弃置红叉（红叉是自绘位图 40x42，画在格子正中；锁在左上角）",
            v.version
        )),
        Err(e) => log(&format!("dark_lock 不工作：{e}")),
    }
    if dir().join(NO_MENU_TEXT_FILE).exists() {
        NO_MENU_TEXT.store(true, Ordering::Relaxed);
        log("这次启动游戏脚本覆盖包没有加载（有 no_menu_text.txt）：菜单文字不在，物品菜单里不加「锁定 / 送给」（锁图标照常）");
    }
    1
}
