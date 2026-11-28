//! RNG Lands 一键出售。
//!
//! 赐福「暗黑重铸」→「一键出售」选类别（武器 / 防具 / 护符，背包的三类和木箱的三类）和档位（普通 / 稀有及以下 / 史诗及以下 / 传说及以下）确认后，
//! 对话脚本点亮请求标记 SELL_FLAGS[(地方 × 3 + 类别) × 4 + 档位]；这里每帧看这 24 个标记，看到就熄灭它，
//! 把背包里没装备着的（或木箱里的）、这一类这一档及以下的 RNG 装备按游戏里的售价卖掉。
//!   木箱（赐福「整理木箱」，PlayerGameData.storage）和背包是同一种结构（EquipInventoryData），游戏的移除函数只按它自己的字段改
//!   （反汇编 exe+0x24d1e0 看过：格子数组、最近获得列表、壶类计数都在这个结构里，不往外够装备数据），所以木箱也用同一个函数删。
//!   请求标记在官方「予備」的 93000 块（背包 93000~93011）和 92000 块（木箱 92780~92791，2026-11-25 扫过没人用），见 dev\event_flags\说明.md。以前放在 1040490781~792：那一段读档、
//!   换区域时会被写进别的数据（同一个字节被写成角色名之类的值），几个请求一起亮，没点出售也会卖掉东西。后来的 9970~9981 和
//!   RNG Lands 公共事件的事件编号 9970~9973、9977 撞了（事件结束 / 重来会点亮同号标记，9977 是死亡触发的：每死一次卖一次防具）。
//!   防误卖：同一帧亮了不止一个请求（对话脚本一次只点一个）→ 全部熄灭、不卖；进存档后头几秒亮着的（存档里残留的）→ 熄灭、不卖；
//!   人物不在赐福点（赐福菜单开着时一直放 680xx 的坐姿动画）→ 熄灭、不卖（标记是全游戏共用的，别的东西点亮了也不会卖）。
//!   哪些是 RNG 装备：DLL 旁边的 sell_items.txt（重随时自动生成：类别 编号，武器写 +0 的编号）；
//!   稀有度、售价：游戏当时加载的参数（EquipParam 的 rarity 0 普通 / 1 稀有 / 2 史诗 / 3 传说、sellValue）。
//!   移除物品调用游戏自己的函数（和 The Grand Archives CE 表的 RemoveItem 同一个，按特征码找）。
//! 确认框里的预计件数 / 卢恩：sc/sell.py 在确认框文字里留了占位「〔#S<序号> …〕」（英文 [ ]；序号 0~9、A~N），人物坐在赐福点时每 30 帧按背包、木箱现状算好，
//!   交给 dll\dark_core 的取文字钩子（导出函数 dr_msg_confirm，msghook.rs），游戏取这段文字时换成算好的。
//!   以前是扫内存找到占位原地改写：找到的多是临时副本，写进已经释放的内存会写坏游戏的堆（2026-11-18 改掉）。
//! 锁定的装备不卖：%APPDATA%\EldenRing\DarkReforged\角色记录\lock_items.txt（dll\dark_lock 在物品弹出菜单里的「锁定」写的，按类别 + 编号，
//!   武器不含强化等级；还没有这个文件时看 dll\dark_lock\lock_items.txt，旧版写在那）。
//! 每个角色的记录（RNG Lands 自己的事件标记都在 1040490xxx，这一块在游戏里是单独分配的、不在存档的标记表里，重启游戏就全灭了）：
//!   进存档时按记录文件（%APPDATA%\EldenRing\DarkReforged\角色记录\ 下，和配方、存档放一起，整合包换目录 / 更新不丢；
//!   2026-10-29 以前在 EldenRing 根目录、带 DarkReforged_ 前缀，新位置没有时读旧的，见 rec_text）重新点亮，
//!   之后看到变了就改记录、被清掉了就再点亮。记录一行「格子<TAB>数<TAB>角色名」，格子 + 名字都对上才算。
//!   - 局内难度：当前档位现在照 ELDEN RING Reforged 存在会存档的标记 DIFF_CUR0 + 档位（9950 起）里，读档时公共事件按标记重挂效果，
//!     不用 DLL。这里只做一次性迁移：旧版记在 difficulty.txt 里的档位，进存档时点亮一次（当前档 + 请求标记），然后删掉这条记录。
//!     档数看 DLL 旁边的 difficulty_tiers.txt（重随时生成），没有就不管难度。
//!   - 免费许愿（bossres.py 的 patch_free_wishes）：用一次按顺序点亮一个 FREE_FLAG0 + i，次数存在存档里。free_wishes.txt 多一列
//!     「记下时的游戏时间」，只在进下一周目标记被清掉时补回，同格同名的新角色不继承（见 free_from_rec）。
//! 复制战灰（赐福「复制战灰」/ 铁匠修古，原版清单 ShopLineupParam 112000~，重随时写回）：每格由这种战灰的解锁标记打开。
//!   原版公共事件按「战灰原来的获取处」（商店格、光点、Boss）点亮，RNG Lands 把那些地方换成了别的东西，会打开没拿到的战灰，
//!   bossres.py 去掉了这些事件。这里每 2 秒按背包里实际有的战灰开关：有 → 打开（并解锁菜单 ASH_MENU_FLAG），没有 → 关掉。
//!   对照表 DLL 旁边的 ash_dup.txt（重随时生成，一行「战灰编号<TAB>解锁标记」），没有就不管。
//! F9：只读，把背包、仓库里的每一件东西写进 DLL 旁边的 inventory.txt（排查用）。
//! 认 eldenring.exe 2.7.1.0（1.17.1）和 2.7.2.0（1.17.2）（地址表在 ../vendor 的 fromsoftware-rs 副本里）；
//! 别的版本什么都不做，只在 dark_sell.log 里记一行。

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use eldenring::cs::{
    CSEventFlagMan, CSFD4VirtualMemoryFlag, CSTaskGroupIndex, CSTaskImp, EquipInventoryData, EquipParamAccessory, EquipParamProtector,
    EquipParamWeapon, GameDataMan, GameMan, PlayerGameData, SoloParamRepository, WorldChrMan,
};
use eldenring::fd4::FD4TaskData;
use eldenring::util::input;
use fromsoftware_shared::{FromStatic, SharedTaskImpExt};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW, GetProcAddress};

const KEY_DUMP: i32 = 0x78; // F9
const MAX_RUNES: u32 = 999_999_999;
/// 请求标记 = SELL_FLAGS[(地方（0 背包 / 1 木箱）× CATS + 类别（0 武器 / 1 防具 / 2 护符）) × TIERS + 档位（最高稀有度 0 普通 / 1 稀有 / 2 史诗 / 3 传说）]
/// （和 sc/sell.py 的 FLAGS 一致，见 combo）。背包 93000~93011、木箱 92780~92791：官方定义里「予備」的 93000 / 92000 块，
/// 原版和各包的事件 / 对话 / 参数、事件编号都没用（dev\event_flags\说明.md）
const SELL_FLAGS: [u32; COMBOS] = [
    93000, 93001, 93002, 93003, 93004, 93005, 93006, 93007, 93008, 93009, 93010, 93011, //
    92780, 92781, 92782, 92783, 92784, 92785, 92786, 92787, 92788, 92789, 92790, 92791,
];
/// 「出售全部弃置」的请求标记（用户 2026-11-26，sc/sell.py 第 7 项）：单独一个，不进上面 24 个组合
/// （93019 原为圆顶请求、已停用，dev\event_flags\说明.md 明确「没别人用，可以再分配」。
///  最初用的 93012 和许愿石请求撞了，2026-11-26 首次实测踩到：点回收弃置被这里抢先卖了）
const SELL_DISCARD_FLAG: u32 = 93019;
const TIERS: usize = 4;
const CATS: usize = 3;
/// 0 背包 / 1 木箱
const PLACES: usize = 2;
const COMBOS: usize = PLACES * CATS * TIERS;

/// 请求 / 确认框序号 i → (地方, 类别, 档位)
fn combo(i: usize) -> (usize, u8, u8) {
    (i / (CATS * TIERS), (i / TIERS % CATS) as u8, (i % TIERS) as u8)
}
/// 进存档后多少帧内亮着的出售请求算存档里残留的（对话脚本不可能这么快点亮）
const SELL_SETTLE_FRAMES: u32 = 300;
/// 这次进存档后过了几帧（回到标题画面清零）
static INGAME_FRAMES: AtomicU32 = AtomicU32::new(0);

/// 游戏的「从背包移除物品」：(EquipInventoryData, 全局格子序号, 数量)。
/// 特征码出自 The Grand Archives 的 RemoveItem（命中位置再往前 16 字节是函数开头）
const REMOVE_ITEM_AOB: [Option<u8>; 13] = [
    None, Some(0x83), Some(0xEC), None, Some(0x8B), Some(0xF2), None, Some(0x8B), Some(0xE9), None, Some(0x85), Some(0xC0), Some(0x74),
];
const REMOVE_ITEM_BACK: usize = 16;
type RemoveItemFn = unsafe extern "C" fn(*mut EquipInventoryData, u32, u32) -> u64;

/// 局内难度的标记（和 bossres.py 的 DIFF_REQ0 / DIFF_CUR0 一致）：请求 = DIFF_REQ0 + 档位，当前 = DIFF_CUR0 + 档位，都在会存档的 99xx
/// （请求以前在 1040490720 起，那一段会被乱写）
const DIFF_REQ0: u32 = 9960;
const DIFF_CUR0: u32 = 9950;
const DIFF_MAX: usize = 10;
/// 免费许愿用掉的次数 = 从 FREE_FLAG0 起连续点亮的标记数（和 bossres.py 的 FREE_FLAG0 / FREE_MAX 一致）。
/// 93020~93049 在官方「予備」的 93000 块（会存档）；以前在 1040490600~629（不存档、会被写坏），记录文件照样按角色记、进存档时点亮，旧存档靠它迁过来
const FREE_FLAG0: u32 = 93020;
const FREE_MAX: usize = 30;
const DIFF_FILE: &str = "difficulty.txt";
const DIFF_HEAD: &str = "# 旧版记的每个角色的局内难度（现在难度存在存档里；dark_sell 进存档时迁移一次就删掉这一行）：格子\t档位（0 起）\t角色名\n";
const FREE_FILE: &str = "free_wishes.txt";
const FREE_HEAD: &str = "# 每个角色用掉的免费许愿次数（dark_sell 写；次数存在存档里，这里只在进下一周目标记被清掉时补回，删掉这个文件不会重新送）：格子\t次数\t角色名\t记下时的游戏时间（毫秒）\n";

/// 这次进存档的角色和按记录应有的状态；回到标题画面清掉
struct Progress {
    who: (i32, String),
    /// 免费许愿用掉的次数
    free: usize,
    /// 标记被清掉后重新点亮过没有（只记一次日志）
    relit: bool,
}
static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);
/// 复制战灰：赐福菜单「复制战灰」的解锁标记（原版拿到第一个战灰时点亮）
const ASH_MENU_FLAG: u32 = 65800;
/// 对照表（战灰编号, 解锁标记），进程里读一次
static ASH_TABLE: OnceLock<Vec<(u32, u32)>> = OnceLock::new();
/// 这次进存档后对过一次复制战灰没有（第一次记一行总数，之后只在有变化时记）；回到标题画面清掉
static ASH_SEEN: AtomicBool = AtomicBool::new(false);
static DIR: OnceLock<PathBuf> = OnceLock::new();
static REMOVE_ITEM: OnceLock<Option<usize>> = OnceLock::new();
static RNG_ITEMS: OnceLock<HashSet<(u8, u32)>> = OnceLock::new();
/// 确认框各序号上次交给 dark_core 的文字（没变就不再交）
/// 确认框里的预计文字（24 个组合 + 1 个「出售全部弃置」，见 write_fields）
static FIELD_TEXT: Mutex<[String; COMBOS + 1]> = Mutex::new([const { String::new() }; COMBOS + 1]);
/// dark_core 的 dr_msg_confirm 的地址（0 = 还没找到）；找不到时记过一行日志没有
static CONFIRM_FN: AtomicUsize = AtomicUsize::new(0);
static CONFIRM_TOLD: AtomicBool = AtomicBool::new(false);

fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."))
}

/// 角色记录的文件夹 %APPDATA%\EldenRing\DarkReforged\角色记录（和存档、启动前备份放一起，整合包换目录 / 更新都不丢）；没有 APPDATA 就用 DLL 所在目录。
/// 联机（Seamless Co-op 加载了）时是里面的 联机\（见 ersc_loaded）
fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("EldenRing").join("DarkReforged").join("角色记录")).unwrap_or_else(dir);
    if ersc_loaded() { base.join("联机") } else { base }
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

/// 读一个角色记录：新位置没有就读旧版放在 %APPDATA%\EldenRing 根目录的 DarkReforged_<名字>（2026-10-29 以前；启动器会搬过来，这里兜底）。
/// 联机时不兜底（旧位置的是单机角色的）。写总是写新位置
fn rec_text(name: &str) -> std::io::Result<String> {
    fs::read_to_string(data_dir().join(name)).or_else(|e| match std::env::var_os("APPDATA") {
        Some(a) if !ersc_loaded() => fs::read_to_string(PathBuf::from(a).join("EldenRing").join(format!("DarkReforged_{name}"))),
        _ => Err(e),
    })
}

fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir().join("dark_sell.log")) {
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

/// 版本不对时库里查地址表会 panic（GameDataMan 要用地址表找）：在这里接住
fn version_ok() -> bool {
    catch_unwind(|| {
        let _ = unsafe { GameDataMan::instance() };
    })
    .is_ok()
}

/// 在游戏主程序的可执行节里找特征码，要求只命中一处
fn aob_scan_unique(pat: &[Option<u8>]) -> Result<usize, String> {
    let base = unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandle：{e}"))?.0 as usize;
    unsafe {
        let pe = base + *((base + 0x3C) as *const u32) as usize;
        let nsec = *((pe + 6) as *const u16) as usize;
        let opt_size = *((pe + 20) as *const u16) as usize;
        let sec0 = pe + 24 + opt_size;
        let mut hits = Vec::new();
        for s in 0..nsec {
            let sh = sec0 + 40 * s;
            let chars = *((sh + 36) as *const u32);
            if chars & 0x2000_0000 == 0 {
                continue; // 不可执行
            }
            let va = base + *((sh + 12) as *const u32) as usize;
            let size = *((sh + 8) as *const u32) as usize;
            let mem = std::slice::from_raw_parts(va as *const u8, size);
            'outer: for i in 0..size.saturating_sub(pat.len()) {
                for (j, p) in pat.iter().enumerate() {
                    if let Some(b) = p {
                        if mem[i + j] != *b {
                            continue 'outer;
                        }
                    }
                }
                hits.push(va + i);
                if hits.len() > 1 {
                    return Err(format!("命中不止一处（{:#x}、{:#x}）", hits[0] - base, hits[1] - base));
                }
            }
        }
        match hits.first() {
            Some(&a) => Ok(a),
            None => Err("没找到".into()),
        }
    }
}

fn remove_item_fn() -> Option<usize> {
    *REMOVE_ITEM.get_or_init(|| match aob_scan_unique(&REMOVE_ITEM_AOB) {
        Ok(hit) => {
            let f = hit - REMOVE_ITEM_BACK;
            let base = unsafe { GetModuleHandleW(None) }.map(|h| h.0 as usize).unwrap_or(0);
            let head = unsafe { std::slice::from_raw_parts(f as *const u8, 24) };
            log(&format!("移除物品函数：exe+{:#x}，开头 {:02x?}", f - base, head));
            Some(f)
        }
        Err(e) => {
            log(&format!("移除物品函数特征码：{e}，出售不可用"));
            None
        }
    })
}

fn rng_items() -> &'static HashSet<(u8, u32)> {
    RNG_ITEMS.get_or_init(|| {
        let mut m = HashSet::new();
        match fs::read_to_string(dir().join("sell_items.txt")) {
            Ok(t) => {
                for line in t.lines() {
                    let f: Vec<&str> = line.split('\t').collect();
                    if f.len() < 2 || line.starts_with('#') {
                        continue;
                    }
                    if let (Ok(c), Ok(i)) = (f[0].parse::<u8>(), f[1].parse::<u32>()) {
                        m.insert((c, i));
                    }
                }
                log(&format!("RNG 装备名单：{} 件", m.len()));
            }
            Err(e) => log(&format!("读不到 RNG 装备名单 sell_items.txt（先用重随版启动一次）：{e}")),
        }
        m
    })
}

/// 游戏当时加载的参数里的（稀有度, 售价）
fn rarity_price(cat: u8, pid: u32) -> Option<(u8, i32)> {
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    match cat {
        0 => repo.get::<EquipParamWeapon>(pid - pid % 100).map(|p| (p.rarity(), p.sell_value())),
        1 => repo.get::<EquipParamProtector>(pid).map(|p| (p.rarity(), p.sell_value())),
        2 => repo.get::<EquipParamAccessory>(pid).map(|p| (p.rarity(), p.sell_value())),
        _ => None,
    }
}

fn equipped_ids(pgd: &PlayerGameData) -> Vec<u32> {
    let e = &pgd.equipment.equipment_entries;
    let mut v = vec![
        e.weapon_primary_left.into_inner(),
        e.weapon_primary_right.into_inner(),
        e.weapon_secondary_left.into_inner(),
        e.weapon_secondary_right.into_inner(),
        e.weapon_tertiary_left.into_inner(),
        e.weapon_tertiary_right.into_inner(),
        e.protector_head.into_inner(),
        e.protector_chest.into_inner(),
        e.protector_hands.into_inner(),
        e.protector_legs.into_inner(),
    ];
    // covenant：dark_slot 打开的第五个护符槽
    for a in e.accessories.iter().chain(std::iter::once(&e.covenant)) {
        v.push(a.into_inner());
    }
    v
}

/// 出售名单用的键：武器去掉强化等级（编号末两位）
fn sell_key(cat: u8, pid: u32) -> (u8, u32) {
    if cat == 0 { (0, pid - pid % 100) } else { (cat, pid) }
}

fn dump_list(out: &mut String, name: &str, inv: &EquipInventoryData, equipped: &[u32]) -> (u32, u32) {
    let mut n = 0;
    let mut gear = 0;
    let _ = writeln!(out, "## {name}");
    for it in inv.items_data.items() {
        let raw = it.item_id.into_inner();
        let cat = it.item_id.category() as u8;
        let pid = it.item_id.param_id();
        let kind = match cat {
            0 => "武器",
            1 => "防具",
            2 => "护符",
            4 => "道具",
            8 => "战灰",
            _ => "?",
        };
        if cat <= 2 {
            gear += 1;
        }
        let eq = if equipped.contains(&raw) { "装备中" } else { "" };
        let _ = writeln!(out, "{kind}\t{pid}\t数量 {}\t{eq}\traw {raw:#010x}", it.quantity);
        n += 1;
    }
    (n, gear)
}

fn dump() {
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else {
        log("F9：还没进存档（GameDataMan 不在）");
        return;
    };
    let pgd: &PlayerGameData = &gdm.main_player_game_data;
    let equipped = equipped_ids(pgd);
    let mut out = String::new();
    let _ = writeln!(out, "卢恩 {}  等级 {}", pgd.rune_count, pgd.level);
    let _ = writeln!(out, "装备中（raw）：{:x?}", equipped);
    let _ = writeln!(out, "装备格子序号：{:?}", pgd.equipment.equipment_item_idx_list);
    let (n1, g1) = dump_list(&mut out, "背包", &pgd.equipment.equip_inventory_data, &equipped);
    let (n2, g2) = match &pgd.storage {
        Some(st) => dump_list(&mut out, "仓库", st, &[]),
        None => (0, 0),
    };
    let path = dir().join("inventory.txt");
    match fs::write(&path, out) {
        Ok(_) => log(&format!("F9：背包 {n1} 格（装备类 {g1}），仓库 {n2} 格（装备类 {g2}）→ {}", path.display())),
        Err(e) => log(&format!("F9：写文件失败 {e}")),
    }
}

/// 背包结构（和 TGA 从 PlayerGameData+0x5D0 读出的地址核对过）
fn inventory(pgd: &mut PlayerGameData) -> Option<*mut EquipInventoryData> {
    let inv_ptr = &mut pgd.equipment.equip_inventory_data as *mut EquipInventoryData;
    let tga_ptr = unsafe { *((pgd as *const PlayerGameData as *const u8).add(0x5D0) as *const usize) };
    if tga_ptr != inv_ptr as usize {
        log(&format!("背包结构地址对不上（库 {:#x}，PlayerGameData+0x5D0 {:#x}）", inv_ptr as usize, tga_ptr));
        return None;
    }
    Some(inv_ptr)
}

/// 这个存档格子里锁定的装备（dark_lock 写的 角色记录\lock_items.txt，见 data_dir；还没有就看旧版的 dll\dark_lock\lock_items.txt）：
/// {sell_key}（武器不含强化等级，强化后仍算锁定）。
/// 一行「格子<TAB>类别（0 武器 / 1 防具 / 2 护符）<TAB>编号<TAB>角色名」；旧版按句柄记的行「格子<TAB>句柄<TAB>编号」（十六进制）
/// 按句柄的类别换算。句柄强化、读档后会换，不能按句柄认。每次现读，锁定后马上生效
fn locked_keys() -> HashSet<(u8, u32)> {
    let Some(slot) = unsafe { GameMan::instance() }.ok().map(|g| g.save_slot) else { return HashSet::new() };
    let old = dir().parent().map(|d| d.join("dark_lock").join("lock_items.txt")).unwrap_or_default();
    let text = rec_text("lock_items.txt").or_else(|e| if ersc_loaded() { Err(e) } else { fs::read_to_string(old) }).unwrap_or_default();
    text.lines().filter_map(|l| lock_line(l, slot)).collect()
}

/// 弃置名单（dark_core 的 gear.rs 写的，工坊「超限」页武器栏第 4 个按钮）：这个角色的生成武器 base 编号。
/// 键是 base（param_id / 10000 * 10000，每把生成武器独占一条参数行），所以只跟武器对得上。
/// 这里只读不写（写名单的是 dark_core 和 dark_lock，别两边同时改）。
/// 格子和角色名都对上才算（同一格子换了角色，参数行是另一套装备，按格子认会卖错）
fn discard_keys(pgd: &PlayerGameData) -> HashSet<u32> {
    let Some(slot) = unsafe { GameMan::instance() }.ok().map(|g| g.save_slot) else { return HashSet::new() };
    let len = pgd.character_name.iter().position(|&c| c == 0).unwrap_or(pgd.character_name.len());
    let name = String::from_utf16_lossy(&pgd.character_name[..len]);
    discard_parse(&rec_text("discard_items.txt").unwrap_or_default(), slot, &name)
}

/// 名单全文里这个角色（格子 + 角色名）的 base；一行「格子<TAB>0<TAB>base<TAB>角色名」，说明行（#）、缺列的不算
fn discard_parse(text: &str, slot: i32, name: &str) -> HashSet<u32> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            let ours = f.len() >= 4 && f[0].trim_start_matches('\u{feff}').parse::<i32>().ok() == Some(slot) && f[3] == name;
            ours.then(|| f[2].parse::<u32>().ok()).flatten()
        })
        .collect()
}

/// 物品编号（含类别位、含强化等级）→ 生成武器的 base
fn pid_base(raw: u32) -> u32 {
    (raw & 0x0FFF_FFFF) / 10000 * 10000
}

/// 名单的一行，是这个格子的就返回 sell_key
fn lock_line(line: &str, slot: i32) -> Option<(u8, u32)> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 3 || f[0].parse::<i32>().ok() != Some(slot) {
        return None;
    }
    let (cat, id) = if f[1].len() == 8 {
        let cat = match u32::from_str_radix(f[1], 16).ok()? >> 28 {
            0x8 => 0,
            0x9 => 1,
            0xA => 2,
            _ => return None,
        };
        (cat, u32::from_str_radix(f[2], 16).ok()?)
    } else {
        (f[1].parse::<u8>().ok().filter(|c| *c <= 2)?, f[2].parse::<u32>().ok()?)
    };
    Some(sell_key(cat, id & 0x0FFF_FFFF))
}

/// 木箱（赐福「整理木箱」）的结构；还没建好（没进存档）是 None
fn storage(pgd: &mut PlayerGameData) -> Option<*mut EquipInventoryData> {
    pgd.storage.as_ref().map(|s| s.as_ptr())
}

/// 地方（0 背包 / 1 木箱）的结构
fn place_inv(pgd: &mut PlayerGameData, place: usize) -> Option<*mut EquipInventoryData> {
    if place == 0 { inventory(pgd) } else { storage(pgd) }
}

/// 一件能卖的
struct Pick {
    idx: usize,
    raw: u32,
    qty: u32,
    price: u32,
    cat: u8,
    rarity: u8,
}

/// 能卖的：这个结构里没锁定的 RNG 装备（武器 / 防具 / 护符，售价 > 0）；背包的还要没装备着。
/// stored = 木箱：装备着的格子序号是背包的，不拿来比（同号的木箱格子不是装备着的）；编号和身上装备的一样的照样不卖
fn sellable(pgd: &PlayerGameData, inv: &EquipInventoryData, stored: bool, locked: &HashSet<(u8, u32)>) -> Vec<Pick> {
    let list = rng_items();
    let equipped = equipped_ids(pgd);
    let equipped_idx: HashSet<u32> = if stored { HashSet::new() } else { pgd.equipment.equipment_item_idx_list.iter().copied().collect() };
    let key_cap = inv.items_data.key_items_capacity;
    let mut picks = Vec::new();
    for (idx, e) in inv.items_data.normal_entries().iter().enumerate() {
        let Some(it) = e.as_option() else { continue };
        let cat = it.item_id.category() as u8;
        let raw = it.item_id.into_inner();
        let pid = it.item_id.param_id();
        if cat as usize >= CATS || it.quantity == 0 || equipped_idx.contains(&(key_cap + idx as u32)) || equipped.contains(&raw)
            || locked.contains(&sell_key(cat, pid))
        {
            continue;
        }
        if !list.contains(&sell_key(cat, pid)) {
            continue;
        }
        if let Some((rarity, price)) = rarity_price(cat, pid) {
            if price > 0 {
                picks.push(Pick { idx, raw, qty: it.quantity, price: price as u32, cat, rarity });
            }
        }
    }
    picks
}

/// 每类每档的预计（件数, 卢恩）：档位 t = 稀有度 <= t 的合计
fn tally(picks: &[Pick]) -> [[(u32, u32); TIERS]; CATS] {
    let mut out = [[(0u32, 0u32); TIERS]; CATS];
    for p in picks {
        for t in p.rarity as usize..TIERS {
            let e = &mut out[p.cat as usize][t];
            e.0 += p.qty;
            e.1 = e.1.saturating_add(p.price.saturating_mul(p.qty));
        }
    }
    out
}

/// 卖掉地方 place（0 背包 / 1 木箱）里类别 sell_cat、稀有度 <= max_rarity 的，返回（件数, 卢恩）
fn sell_batch(place: usize, sell_cat: u8, max_rarity: u8) -> Option<(u32, u32)> {
    let gdm = unsafe { GameDataMan::instance_mut() }.ok()?;
    let f = remove_item_fn()?;
    let pgd: &mut PlayerGameData = &mut gdm.main_player_game_data;
    let inv_ptr = place_inv(pgd, place)?;
    let inv = unsafe { &mut *inv_ptr };
    let key_cap = inv.items_data.key_items_capacity;
    let picks: Vec<(usize, u32, u32, u32)> = sellable(pgd, inv, place == 1, &locked_keys())
        .into_iter()
        .filter(|p| p.cat == sell_cat && p.rarity <= max_rarity)
        .map(|p| (p.idx, p.raw, p.qty, p.price))
        .collect();
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    let (mut n, mut runes, mut fail) = (0u32, 0u32, 0u32);
    for (idx, raw, qty, price) in picks {
        let still = inv.items_data.normal_entries()[idx].as_option().filter(|it| it.item_id.into_inner() == raw).map(|it| it.quantity);
        if still != Some(qty) {
            fail += 1;
            continue;
        }
        unsafe { remove(inv_ptr, key_cap + idx as u32, qty) };
        let after = inv.items_data.normal_entries()[idx].as_option().filter(|it| it.item_id.into_inner() == raw).map(|it| it.quantity);
        let sold = qty - after.unwrap_or(0).min(qty);
        if sold == 0 {
            fail += 1;
            continue;
        }
        n += sold;
        runes = runes.saturating_add(price.saturating_mul(sold));
    }
    pgd.rune_count = pgd.rune_count.saturating_add(runes).min(MAX_RUNES);
    if fail > 0 {
        log(&format!("出售：{fail} 件没卖掉（格子变了或移除失败）"));
    }
    Some((n, runes))
}

/// 「出售全部弃置」（用户 2026-11-26，sc/sell.py 的第 7 项）：背包里命中弃置名单的装备按售价卖掉。
/// 和 sell_batch 同一套（RNG 名单、装备中/锁定排除、售价、移除、加卢恩），只把筛选条件从「类别 + 稀有度档」换成「命中弃置名单」；
/// 只背包（弃置只能从工坊打，工坊只列背包；木箱那三类完全不受影响）
fn sell_discarded() -> Option<(u32, u32)> {
    let gdm = unsafe { GameDataMan::instance_mut() }.ok()?;
    let f = remove_item_fn()?;
    let pgd: &mut PlayerGameData = &mut gdm.main_player_game_data;
    let inv_ptr = place_inv(pgd, 0)?;
    let inv = unsafe { &mut *inv_ptr };
    let key_cap = inv.items_data.key_items_capacity;
    let disc = discard_keys(pgd);
    let picks: Vec<(usize, u32, u32, u32)> = sellable(pgd, inv, false, &locked_keys())
        .into_iter()
        .filter(|p| p.cat == 0 && disc.contains(&pid_base(p.raw))) // 名单只记武器，防具、护符的编号别碰巧对上
        .map(|p| (p.idx, p.raw, p.qty, p.price))
        .collect();
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    let (mut n, mut runes, mut fail) = (0u32, 0u32, 0u32);
    for (idx, raw, qty, price) in picks {
        let still = inv.items_data.normal_entries()[idx].as_option().filter(|it| it.item_id.into_inner() == raw).map(|it| it.quantity);
        if still != Some(qty) {
            fail += 1;
            continue;
        }
        unsafe { remove(inv_ptr, key_cap + idx as u32, qty) };
        let after = inv.items_data.normal_entries()[idx].as_option().filter(|it| it.item_id.into_inner() == raw).map(|it| it.quantity);
        let sold = qty - after.unwrap_or(0).min(qty);
        if sold == 0 {
            fail += 1;
            continue;
        }
        n += sold;
        runes = runes.saturating_add(price.saturating_mul(sold));
    }
    pgd.rune_count = pgd.rune_count.saturating_add(runes).min(MAX_RUNES);
    if fail > 0 {
        log(&format!("出售（全部弃置）：{fail} 件没卖掉（格子变了或移除失败）"));
    }
    Some((n, runes))
}

fn group_digits(v: u32) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

type ConfirmFn = unsafe extern "C" fn(u8, u8, *const u16, usize, *const u16, usize);

/// dark_core 的取文字钩子收确认框文字的函数（dark_core 比我们先加载；没找到下次再找）
fn confirm_fn() -> Option<ConfirmFn> {
    let mut a = CONFIRM_FN.load(Ordering::Relaxed);
    if a == 0 {
        let m = unsafe { GetModuleHandleW(windows::core::w!("dark_core.dll")) }.ok()?;
        a = unsafe { GetProcAddress(m, windows::core::s!("dr_msg_confirm")) }? as usize;
        CONFIRM_FN.store(a, Ordering::Relaxed);
    }
    Some(unsafe { std::mem::transmute::<usize, ConfirmFn>(a) })
}

/// 一键出售确认框的预计结果（序号 i 见 combo）：中英文各一份交给 dark_core，变了才交。
/// 确认框只在赐福菜单里：人物不在赐福点就不算（木箱最多两千来格，不用一直扫），坐下后半秒内就算好
fn write_fields() {
    let Ok(gdm) = (unsafe { GameDataMan::instance_mut() }) else { return };
    let pgd: &mut PlayerGameData = &mut gdm.main_player_game_data;
    if pgd.level == 0 || !at_grace(player_anim()) {
        return;
    }
    let Some(publish) = confirm_fn() else {
        if !CONFIRM_TOLD.swap(true, Ordering::Relaxed) {
            log("一键出售确认框：找不到 dark_core 的 dr_msg_confirm（dark_core 没加载？），确认框里显示占位的提示");
        }
        return;
    };
    let locked = locked_keys();
    let mut by_place = [[[(0u32, 0u32); TIERS]; CATS]; PLACES];
    for (place, out) in by_place.iter_mut().enumerate() {
        let Some(inv_ptr) = place_inv(pgd, place) else {
            if place == 0 {
                return; // 背包结构对不上：什么都不交（确认框显示占位的提示）
            }
            continue; // 木箱还没有：算没有可卖的
        };
        let inv = unsafe { &*inv_ptr };
        *out = tally(&sellable(pgd, inv, place == 1, &locked));
    }
    let est: [(u32, u32); COMBOS] = std::array::from_fn(|i| {
        let (place, cat, tier) = combo(i);
        by_place[place][cat as usize][tier as usize]
    });
    let mut last = FIELD_TEXT.lock().unwrap_or_else(|e| e.into_inner());
    for (i, &(n, r)) in est.iter().enumerate() {
        let (zh, en) = if n == 0 {
            ("没有可卖的".to_string(), "Nothing to sell".to_string())
        } else {
            (format!("预计卖出 {n} 件，约 {} 卢恩", group_digits(r)), format!("Est. {n} items for {} runes", group_digits(r)))
        };
        let key = format!("{zh}|{en}");
        if last[i] == key {
            continue;
        }
        let (z, e): (Vec<u16>, Vec<u16>) = (zh.encode_utf16().collect(), en.encode_utf16().collect());
        unsafe { publish(b'S', i as u8, z.as_ptr(), z.len(), e.as_ptr(), e.len()) };
        last[i] = key;
    }
    // 第 25 项（#SO）：「出售全部弃置」的预计（只背包、命中弃置名单的）
    let (dn, dr) = {
        let mut acc = (0u32, 0u32);
        if let Some(inv_ptr) = place_inv(pgd, 0) {
            let inv = unsafe { &*inv_ptr };
            let disc = discard_keys(pgd);
            for p in sellable(pgd, inv, false, &locked) {
                if p.cat == 0 && disc.contains(&pid_base(p.raw)) {
                    acc.0 += p.qty;
                    acc.1 = acc.1.saturating_add(p.price.saturating_mul(p.qty));
                }
            }
        }
        acc
    };
    let (zh, en) = if dn == 0 {
        ("没有可出售的弃置武器".to_string(), "No discarded items to sell".to_string())
    } else {
        (format!("预计卖出 {dn} 件，约 {} 卢恩", group_digits(dr)), format!("Est. {dn} items for {} runes", group_digits(dr)))
    };
    let key = format!("{zh}|{en}");
    if last[COMBOS] != key {
        let (z, e): (Vec<u16>, Vec<u16>) = (zh.encode_utf16().collect(), en.encode_utf16().collect());
        unsafe { publish(b'S', COMBOS as u8, z.as_ptr(), z.len(), e.as_ptr(), e.len()) };
        last[COMBOS] = key;
    }
}

/// 局内难度的定义（difficulty_tiers.txt 第一行非注释行「档数<TAB>普通是第几档<TAB>各档名字…」）：（档数, 普通, 名字）
fn parse_diff_def(text: &str) -> Option<(usize, usize, Vec<String>)> {
    let line = text.lines().find(|l| !l.starts_with('#') && !l.trim().is_empty())?;
    let f: Vec<&str> = line.split('\t').collect();
    let n = f.first()?.trim().parse::<usize>().ok().filter(|n| (1..=DIFF_MAX).contains(n))?;
    let normal = f.get(1)?.trim().parse::<usize>().ok().filter(|k| *k < n)?;
    Some((n, normal, f[2..].iter().map(|s| s.trim().to_string()).collect()))
}

/// 记录（一行「格子<TAB>数<TAB>角色名」）里这个角色的数：格子、名字都对上才算（格子里换了新角色不沿用）
fn rec_get(text: &str, who: &(i32, String)) -> Option<usize> {
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0) && f[2] == who.1 { f[1].parse::<usize>().ok() } else { None }
    })
}

fn rec_read(file: &str, who: &(i32, String)) -> Option<usize> {
    rec_text(file).ok().and_then(|t| rec_get(&t, who))
}

/// 免费许愿记录里这个角色的（次数, 记下时的游戏时间）；老记录没有时间这一列
fn free_rec(text: &str, who: &(i32, String)) -> Option<(usize, Option<u32>)> {
    let f = text.lines().map(|l| l.split('\t').collect::<Vec<_>>()).find(|f| f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0) && f[2] == who.1)?;
    Some((f[1].parse().ok()?, f.get(3).and_then(|t| t.parse().ok())))
}

/// 进存档时定这个角色用掉了几次免费许愿。次数存在存档的标记里，记录只在标记被清掉（进下一周目）时补回，而且只认同一个角色：
/// 游戏里没有角色 id，但游戏时间按角色记、只增不减；现在比记下时还早，说明格子里已经是同名的新角色（或者读了更早的备份），只信存档。
/// 没有时间的老记录也不用：写它的旧版进存档时已经把次数点亮、存进存档了
fn free_from_rec(rec: Option<(usize, Option<u32>)>, used: usize, now: u32) -> usize {
    match rec {
        Some((n, Some(t))) if now >= t => n.max(used),
        _ => used,
    }
}

/// 去掉这个角色的记录（格子、名字都对上的行），别的保留
fn rec_remove(old: &str, head: &str, who: &(i32, String)) -> String {
    let mut out = head.to_string();
    for l in old.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = l.split('\t').collect();
        if !(f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0) && f[2] == who.1) {
            out += l;
            out.push('\n');
        }
    }
    out
}

/// 改这个角色的记录：同一个格子的旧行（包括格子里以前的角色）去掉，加一行新的
fn rec_update(old: &str, head: &str, who: &(i32, String), v: usize, play_time: u32) -> String {
    let mut out = head.to_string();
    for l in old.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if l.split('\t').next().and_then(|s| s.parse::<i32>().ok()) != Some(who.0) {
            out += l;
            out.push('\n');
        }
    }
    let _ = writeln!(out, "{}\t{}\t{}\t{}", who.0, v, who.1, play_time);
    out
}

fn rec_save(file: &str, head: &str, who: &(i32, String), v: usize, play_time: u32) {
    rec_write(file, |old| rec_update(old, head, who, v, play_time));
}

fn rec_write(file: &str, f: impl FnOnce(&str) -> String) {
    let _ = fs::create_dir_all(data_dir());
    let p = data_dir().join(file);
    let tmp = data_dir().join(format!("{file}.tmp"));
    let out = f(&rec_text(file).unwrap_or_default());
    if fs::write(&tmp, out).and_then(|_| fs::rename(&tmp, &p)).is_err() {
        log(&format!("写 {file} 失败"));
    }
}

/// 点亮这一档的「当前」标记（熄灭其他档）和请求标记（公共事件看到后清掉各档的效果、挂上这一档、熄灭请求）：只在迁移旧记录时用
fn diff_apply(vm: &mut CSFD4VirtualMemoryFlag, n: usize, k: usize) {
    for j in 0..n {
        vm.set_flag(DIFF_CUR0 + j as u32, j == k);
        vm.set_flag(DIFF_REQ0 + j as u32, j == k);
    }
}

/// 免费许愿用掉的次数：从 FREE_FLAG0 起连续点亮了几个
fn free_used(vm: &CSFD4VirtualMemoryFlag) -> usize {
    (0..FREE_MAX).take_while(|&j| vm.get_flag(FREE_FLAG0 + j as u32)).count()
}

fn free_apply(vm: &mut CSFD4VirtualMemoryFlag, used: usize) {
    for j in 0..used.min(FREE_MAX) {
        vm.set_flag(FREE_FLAG0 + j as u32, true);
    }
}

/// 每个角色的记录：进存档时把旧版的难度记录迁移进存档标记（一次）、按记录点亮免费许愿的标记；之后许愿用掉了免费次数就改记录，
/// 免费次数比记下的少（标记被清掉了）就再点亮
fn progress_tick() {
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else { return };
    let pgd: &PlayerGameData = &gdm.main_player_game_data;
    let mut st = PROGRESS.lock().unwrap();
    if pgd.level == 0 {
        *st = None;
        return;
    }
    let Some(slot) = unsafe { GameMan::instance() }.ok().map(|g| g.save_slot).filter(|s| *s >= 0) else { return };
    let len = pgd.character_name.iter().position(|&c| c == 0).unwrap_or(pgd.character_name.len());
    let who = (slot, String::from_utf16_lossy(&pgd.character_name[..len]));
    // 标题画面、捏人时名字是空的：这时还不是哪个角色
    if who.1.is_empty() {
        *st = None;
        return;
    }
    let Ok(flags) = (unsafe { CSEventFlagMan::instance_mut() }) else { return };
    let vm = &mut flags.virtual_memory_flag;
    let name = |names: &[String], k: usize| names.get(k).cloned().unwrap_or_else(|| format!("第 {k} 档"));
    match st.as_mut() {
        Some(p) if p.who == who => {
            let mut relit = Vec::new();
            let used = free_used(vm);
            if used > p.free {
                p.free = used;
                rec_save(FREE_FILE, FREE_HEAD, &who, used, gdm.play_time);
                log(&format!("免费许愿：{} 用掉了 {used} 次", who.1));
            } else if used < p.free {
                free_apply(vm, p.free);
                relit.push(format!("免费许愿用掉 {} 次", p.free));
            }
            if !relit.is_empty() && !p.relit {
                p.relit = true;
                log(&format!("标记被清掉了，按记录重新点亮：{}", relit.join("、")));
            }
        }
        _ => {
            let def = fs::read_to_string(dir().join("difficulty_tiers.txt")).ok().and_then(|t| parse_diff_def(&t));
            let mut msg = Vec::new();
            if let Some((n, _normal, names)) = def {
                match rec_read(DIFF_FILE, &who).filter(|k| *k < n) {
                    Some(k) => {
                        diff_apply(vm, n, k);
                        rec_write(DIFF_FILE, |old| rec_remove(old, DIFF_HEAD, &who));
                        msg.push(format!("难度按旧版记录迁移成「{}」（以后存在存档里）", name(&names, k)));
                    }
                    None => {
                        let cur: Vec<String> = (0..n).filter(|&k| vm.get_flag(DIFF_CUR0 + k as u32)).map(|k| name(&names, k)).collect();
                        msg.push(format!("难度（存档里）：{}", if cur.is_empty() { "还没定，读档事件会按普通".to_string() } else { cur.join("、") }));
                    }
                }
            }
            let rec = rec_text(FREE_FILE).ok().and_then(|t| free_rec(&t, &who));
            let now = gdm.play_time;
            let free = free_from_rec(rec, free_used(vm), now).min(FREE_MAX);
            free_apply(vm, free);
            // 记录不是这个角色的、或者和结论不一样，就改掉
            let keep = match rec {
                Some((n, Some(t))) => n == free && now >= t,
                Some(_) => false,
                None => free == 0,
            };
            if !keep {
                rec_save(FREE_FILE, FREE_HEAD, &who, free, now);
            }
            msg.push(format!("免费许愿已用 {free} 次"));
            if let Some((n, _)) = rec.filter(|r| r.0 > free) {
                msg.push(format!("同格同名的旧记录 {n} 次不算（不是这个角色的）"));
            }
            msg.push(format!("游戏时间 {} 分钟", now / 60_000));
            log(&format!("进入存档（格子 {} {}）：{}", who.0, who.1, msg.join("，")));
            *st = Some(Progress { who, free, relit: false });
        }
    }
}

/// 对照表一行「战灰编号<TAB>解锁标记」，注释、空行跳过
fn parse_ash_table(text: &str) -> Vec<(u32, u32)> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.trim().parse().ok()?, f.next()?.trim().parse().ok()?))
        })
        .collect()
}

/// 背包（和仓库）里有的战灰编号；背包一件东西都没有（还没读进来）就是 None
fn owned_gems(pgd: &PlayerGameData) -> Option<HashSet<u32>> {
    let mut n = 0usize;
    let mut out = HashSet::new();
    let mut add = |inv: &EquipInventoryData| {
        for it in inv.items_data.items() {
            n += 1;
            if it.item_id.category() as u8 == 8 {
                out.insert(it.item_id.param_id());
            }
        }
    };
    add(&pgd.equipment.equip_inventory_data);
    if let Some(st) = &pgd.storage {
        add(st);
    }
    (n > 0).then_some(out)
}

/// 复制战灰：按背包里实际有的战灰开关每格的解锁标记（有 → 开，并解锁菜单；没有 → 关）
fn ash_dup_tick() {
    let table = ASH_TABLE.get_or_init(|| parse_ash_table(&fs::read_to_string(dir().join("ash_dup.txt")).unwrap_or_default()));
    if table.is_empty() {
        return;
    }
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else { return };
    let pgd: &PlayerGameData = &gdm.main_player_game_data;
    if pgd.level == 0 {
        ASH_SEEN.store(false, Ordering::Relaxed);
        return;
    }
    let Some(owned) = owned_gems(pgd) else { return };
    let Ok(flags) = (unsafe { CSEventFlagMan::instance_mut() }) else { return };
    let vm = &mut flags.virtual_memory_flag;
    let (mut on, mut off, mut have) = (0, 0, 0);
    for &(gem, flag) in table {
        let want = owned.contains(&gem);
        have += want as u32;
        if vm.get_flag(flag) != want {
            vm.set_flag(flag, want);
            if want { on += 1 } else { off += 1 }
        }
    }
    if have > 0 && !vm.get_flag(ASH_MENU_FLAG) {
        vm.set_flag(ASH_MENU_FLAG, true);
    }
    if !ASH_SEEN.swap(true, Ordering::Relaxed) {
        log(&format!("复制战灰：进入存档时背包里有 {have} 种（清单 {} 种）；打开 {on} 格、关掉 {off} 格", table.len()));
    } else if on + off > 0 {
        log(&format!("复制战灰：打开 {on} 格、关掉 {off} 格（背包里现在有 {have} 种）"));
    }
}

/// 玩家现在的动画（读不到返回 None）
fn player_anim() -> Option<i32> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let p = wcm.main_player.as_ref()?;
    let ta = &p.chr_ins.modules.time_act;
    Some(ta.anim_queue[ta.read_idx as usize % ta.anim_queue.len()].anim_id)
}

/// 赐福菜单开着时人物坐在赐福点：坐下 68000、坐着 68010~68012 / 68050、起身 68020~68022（dark_wings 的动画日志）；死亡、读档时都不是
fn at_grace(anim: Option<i32>) -> bool {
    matches!(anim, Some(68000..=68099))
}

fn poll_sell() {
    let in_save = unsafe { GameDataMan::instance() }.map(|g| g.main_player_game_data.level != 0).unwrap_or(false);
    let frames = if in_save { INGAME_FRAMES.fetch_add(1, Ordering::Relaxed).saturating_add(1) } else { 0 };
    if !in_save {
        INGAME_FRAMES.store(0, Ordering::Relaxed);
        return;
    }
    let Ok(flags) = (unsafe { CSEventFlagMan::instance_mut() }) else { return };
    const PLACE_NAMES: [&str; PLACES] = ["背包", "木箱"];
    const CAT_NAMES: [&str; CATS] = ["武器", "防具", "护符"];
    const TIER_NAMES: [&str; TIERS] = ["普通", "稀有及以下", "史诗及以下", "传说及以下"];
    let name = |i: usize| {
        let (place, cat, tier) = combo(i);
        format!("{} · {} · {}", PLACE_NAMES[place], CAT_NAMES[cat as usize], TIER_NAMES[tier as usize])
    };
    let lit: Vec<usize> = (0..COMBOS).filter(|&i| flags.virtual_memory_flag.get_flag(SELL_FLAGS[i])).collect();
    // 「出售全部弃置」是单独的标记（sc/sell.py 第 7 项），和 24 个组合一样按「一次只亮一个」处理
    let disc = flags.virtual_memory_flag.get_flag(SELL_DISCARD_FLAG);
    if lit.is_empty() && !disc {
        return;
    }
    for &i in &lit {
        flags.virtual_memory_flag.set_flag(SELL_FLAGS[i], false);
    }
    if disc {
        flags.virtual_memory_flag.set_flag(SELL_DISCARD_FLAG, false);
    }
    let names = || {
        let mut v: Vec<String> = lit.iter().map(|&i| name(i)).collect();
        if disc {
            v.push("全部弃置".to_string());
        }
        v.join("、")
    };
    if frames <= SELL_SETTLE_FRAMES {
        log(&format!("一键出售：进存档时请求标记还亮着（存档里残留的：{}），熄灭、不卖", names()));
        return;
    }
    if lit.len() > 1 || (disc && !lit.is_empty()) {
        log(&format!("一键出售：同时亮了 {} 个请求（{}），不是从菜单点的，全部熄灭、不卖", lit.len() + disc as usize, names()));
        return;
    }
    let anim = player_anim();
    if !at_grace(anim) {
        let a = anim.map(|a| a.to_string()).unwrap_or_else(|| "读不到".to_string());
        log(&format!("一键出售：请求（{}）亮了，但人物不在赐福点（动画 {a}），不是从菜单点的，熄灭、不卖", names()));
        return;
    }
    if disc {
        match catch_unwind(AssertUnwindSafe(sell_discarded)) {
            Ok(Some((n, runes))) => log(&format!("一键出售（全部弃置）：卖了 {n} 件，得到 {runes} 卢恩")),
            Ok(None) => log("一键出售（全部弃置）：没卖（没进存档 / 找不到移除函数 / 地址对不上）"),
            Err(_) => log("一键出售（全部弃置）：出错（已跳过）"),
        }
        return;
    }
    let i = lit[0];
    let (place, cat, max_rarity) = combo(i);
    let name = name(i);
    match catch_unwind(AssertUnwindSafe(|| sell_batch(place, cat, max_rarity))) {
        Ok(Some((n, runes))) => log(&format!("一键出售（{name}）：卖了 {n} 件，得到 {runes} 卢恩")),
        Ok(None) => log(&format!("一键出售（{name}）：没卖（没进存档 / 找不到移除函数 / 地址对不上）")),
        Err(_) => log(&format!("一键出售（{name}）：出错（已跳过）")),
    }
}

/// # Safety
/// 由加载器（YAFSML / me3）调用，不要自己调用。
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(hmodule: u64, reason: u32, _reserved: *mut std::ffi::c_void) -> i32 {
    if reason != 1 {
        return 1;
    }
    let _ = DIR.set(module_dir(hmodule));
    // panic 信息写进日志（游戏没有控制台）；版本检查时的 panic 是预期的，只记一行
    std::panic::set_hook(Box::new(|info| log(&format!("panic：{info}"))));
    let t = local_time();
    log(&format!("==== {}-{:02}-{:02} 加载（进程 {}）", t.year, t.month, t.day, std::process::id()));
    std::thread::spawn(|| {
        if !version_ok() {
            log("游戏版本不是 2.7.1.0（1.17.1）/ 2.7.2.0（1.17.2），dark_sell 不工作");
            return;
        }
        // 加载器注入得很早：任务调度器可能还没建好，失败了每秒重试（最多 10 分钟），把原因记下来
        let mut task = None;
        let mut last = String::new();
        for _ in 0..600 {
            match CSTaskImp::wait_for_instance(Duration::from_secs(1)) {
                Ok(t) => {
                    task = Some(t);
                    break;
                }
                Err(e) => {
                    let msg = format!("{e:?}");
                    if msg != last {
                        log(&format!("等 CSTaskImp：{msg}，继续等"));
                        last = msg;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
        let Some(task) = task else {
            log("10 分钟内等不到 CSTaskImp，dark_sell 不工作");
            return;
        };
        log("dark_sell 已加载：赐福「一键出售」；F9 导出背包（排查用）");
        let mut frame = 0u32;
        let mut sell_errs = 0u32;
        task.run_recurring(
            move |_: &FD4TaskData| {
                frame = frame.wrapping_add(1);
                if frame % 30 == 0 && catch_unwind(AssertUnwindSafe(write_fields)).is_err() {
                    log("写预计数量时出错（已跳过）");
                }
                if frame % 15 == 7 && catch_unwind(AssertUnwindSafe(progress_tick)).is_err() {
                    log("角色记录（难度 / 免费许愿）：读写时出错（已跳过）");
                }
                if frame % 120 == 37 && catch_unwind(AssertUnwindSafe(ash_dup_tick)).is_err() {
                    log("复制战灰：读背包时出错（已跳过）");
                }
                // 任务回调是 extern "C"（vendored shared\src\task.rs），漏出去的 panic 直接带崩游戏：按键查询、一键出售也接住（崩溃风险审查 2026-11-19 第 3 条）
                if catch_unwind(|| input::is_key_pressed(KEY_DUMP)).unwrap_or(false) && catch_unwind(AssertUnwindSafe(dump)).is_err() {
                    log("F9：读背包时出错（已跳过）");
                }
                if catch_unwind(AssertUnwindSafe(poll_sell)).is_err() {
                    sell_errs += 1;
                    if sell_errs <= 3 {
                        log("一键出售：看请求时出错（已跳过这一帧）");
                    }
                }
            },
            CSTaskGroupIndex::FrameBegin,
        );
    });
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_lines() {
        // 旧格式（句柄十六进制）、新格式（类别 + 十进制编号）都换成 sell_key；别的格子、道具、注释行不算
        assert_eq!(lock_line("6	80831123	0bc04530	D", 6), Some((0, 197150000)));
        assert_eq!(lock_line("6	0	197150004	D", 6), Some((0, 197150000)));
        assert_eq!(lock_line("6	1	1234	D", 6), Some((1, 1234)));
        assert_eq!(lock_line("6	a0001f40	20001f40	D", 6), Some((2, 0x1f40)));
        assert_eq!(lock_line("2	0	197150004	D", 6), None);
        assert_eq!(lock_line("6	b0000001	00000001	D", 6), None);
        assert_eq!(lock_line("# 格子	类别	编号	角色名", 6), None);
        // 弃置名单的 base：去掉类别位（高 4 位）和强化等级（level = pid - base，+1 步进）
        assert_eq!(pid_base(0x0E4E1C00), 240_000_000);
        assert_eq!(pid_base(0x8E4E1C00), 240_000_000);
        assert_eq!(pid_base(0x8E4E1C19), 240_000_000);
    }

    #[test]
    fn grace_anims() {
        // 赐福点坐着 / 坐下 / 起身算；死亡、普通动作、读不到都不算
        for a in [68000, 68010, 68011, 68012, 68020, 68021, 68022, 68050] {
            assert!(at_grace(Some(a)));
        }
        for a in [18000, 17140, 63000, 67999, 68100, -1] {
            assert!(!at_grace(Some(a)));
        }
        assert!(!at_grace(None));
        // 93000 块：背包出售 93000~93011、免费许愿 93020~93049；92000 块：木箱出售 92780~92791（92640~92760 死亡惩罚、92770 许愿石十个），互不重复、不重叠
        let s: HashSet<u32> = SELL_FLAGS.iter().copied().collect();
        assert_eq!(s.len(), COMBOS);
        let free = FREE_FLAG0..FREE_FLAG0 + FREE_MAX as u32;
        let half = CATS * TIERS;
        assert!(SELL_FLAGS[..half].iter().all(|f| (93000..93012).contains(f) && !free.contains(f)));
        assert!(SELL_FLAGS[half..].iter().all(|f| (92780..92792).contains(f)));
        assert!(free.start >= 93020 && free.end <= 93050);
        // 「出售全部弃置」是单独一个标记，不能和 24 个组合撞，也不能和许愿石（93012~93017）撞
        // 2026-11-26 实测踩过：用 93012 时点「回收全部弃置」被这边抢先卖了 5 把
        assert!(!s.contains(&SELL_DISCARD_FLAG));
        assert_eq!(SELL_DISCARD_FLAG, 93019);
        assert!(!(93012..=93017).contains(&SELL_DISCARD_FLAG));
    }

    #[test]
    fn combos_and_tally() {
        // 序号 = (地方 × 3 + 类别) × 4 + 档位，和 sc/sell.py 的 i = c × 4 + t（c = 地方 × 3 + 类别）一致
        assert_eq!(combo(0), (0, 0, 0));
        assert_eq!(combo(7), (0, 1, 3));
        assert_eq!(combo(11), (0, 2, 3));
        assert_eq!(combo(12), (1, 0, 0));
        assert_eq!(combo(18), (1, 1, 2));
        assert_eq!(combo(23), (1, 2, 3));
        assert_eq!(SELL_FLAGS[12], 92780);
        // 档位 t 是稀有度 <= t 的合计；稀有度超出 0~3 的不算
        let p = |cat, rarity, qty, price| Pick { idx: 0, raw: 0, qty, price, cat, rarity };
        let t = tally(&[p(0, 0, 1, 100), p(0, 2, 2, 50), p(2, 3, 1, 1000), p(1, 4, 1, 9)]);
        assert_eq!(t[0], [(1, 100), (1, 100), (3, 200), (3, 200)]);
        assert_eq!(t[1], [(0, 0); TIERS]);
        assert_eq!(t[2], [(0, 0), (0, 0), (0, 0), (1, 1000)]);
    }

    #[test]
    fn discard_list_matches_slot_and_name() {
        let t = "# 格子\t类别\t编号\t角色名\r\n0\t0\t1230000\t褪色者\r\n0\t0\t1240000\t别人\n1\t0\t1250000\t褪色者\n0\t0\t1260000\n";
        assert_eq!(discard_parse(t, 0, "褪色者"), HashSet::from([1230000]), "同格子别的角色名、别的格子、缺角色名的都不算");
        assert_eq!(discard_parse(t, 0, "别人"), HashSet::from([1240000]));
        assert!(discard_parse("", 0, "褪色者").is_empty());
    }

    #[test]
    fn ash_table() {
        let t = "# 注释\n10100\t65810\n10200\t65811\n\nxx\t1\n";
        assert_eq!(parse_ash_table(t), vec![(10100, 65810), (10200, 65811)]);
        assert!(parse_ash_table("").is_empty());
    }

    #[test]
    fn progress_files() {
        // 难度定义：跳过注释行；档数 1~10、普通在范围内才算
        let def = "# 注释\n8\t2\t轻松\t休闲\t普通\n";
        assert_eq!(parse_diff_def(def), Some((8, 2, vec!["轻松".into(), "休闲".into(), "普通".into()])));
        assert_eq!(parse_diff_def("8\t8\n"), None);
        assert_eq!(parse_diff_def("0\t0\n"), None);
        assert_eq!(parse_diff_def("# 只有注释\n"), None);
        // 记录：格子、名字都要对上
        let who = (3, "D".to_string());
        let rec = "# 注释\n3\t5\tD\n4\t1\tE\n";
        assert_eq!(rec_get(rec, &who), Some(5));
        assert_eq!(rec_get(rec, &(3, "X".to_string())), None);
        assert_eq!(rec_get(rec, &(4, "E".to_string())), Some(1));
        // 改记录：同格子的旧行（不管名字）去掉，别的格子保留；表头换成新的
        let new = rec_update("# 旧注释\n3\t5\tOld\n4\t1\tE\n", FREE_HEAD, &who, 12, 99);
        assert!(new.starts_with(FREE_HEAD));
        assert_eq!(new.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(), vec!["4\t1\tE", "3\t12\tD\t99"]);
        assert_eq!(rec_get(&new, &who), Some(12));
        // 迁移后删掉这个角色的记录：格子、名字都对上才删
        let rm = rec_remove("# 旧\n3\t5\tD\n3\t1\tOld\n4\t1\tE\n", DIFF_HEAD, &who);
        assert_eq!(rm.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(), vec!["3\t1\tOld", "4\t1\tE"]);
        // 名字里有空格也行（按 TAB 分）
        let sp = (0, "Tarnished One".to_string());
        assert_eq!(rec_get(&rec_update("", DIFF_HEAD, &sp, 4, 0), &sp), Some(4));
    }

    #[test]
    fn free_record_only_for_same_character() {
        let who = (3, "D".to_string());
        assert_eq!(free_rec("# 注\n3\t30\tD\t600000\n", &who), Some((30, Some(600_000))));
        assert_eq!(free_rec("3\t30\tD\n", &who), Some((30, None)));
        assert_eq!(free_rec("3\t30\tX\t1\n", &who), None);
        // 同格同名的新角色：游戏时间比记下时早，不继承
        assert_eq!(free_from_rec(Some((30, Some(600_000))), 0, 3_000), 0);
        // 同一个角色进下一周目、标记被清：按记录补回
        assert_eq!(free_from_rec(Some((30, Some(600_000))), 0, 700_000), 30);
        // 存档里比记录多：以存档为准
        assert_eq!(free_from_rec(Some((5, Some(600_000))), 7, 700_000), 7);
        // 读了更早的备份：只信存档
        assert_eq!(free_from_rec(Some((30, Some(600_000))), 12, 500_000), 12);
        // 没有时间的老记录：只信存档（换了新角色的是 0，旧角色存档里本来就亮着）
        assert_eq!(free_from_rec(Some((30, None)), 0, 3_000), 0);
        assert_eq!(free_from_rec(Some((30, None)), 30, 3_000), 30);
        // 没记录
        assert_eq!(free_from_rec(None, 4, 3_000), 4);
    }
}
