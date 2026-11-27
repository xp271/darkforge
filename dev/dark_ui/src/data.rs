//! 和 dark_core 的接口（它导出的 dr_ws_*，见 dark_core 的 gear\workshop.rs）：找函数、取快照 / 结果 / 重铸范围、解析
use std::collections::{HashMap, HashSet};

use hudhook::windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use hudhook::windows::core::s;

/// dark_core 的接口版本（dr_ws_state 的高 16 位）：2 = 快照带图标；3 = 加「重铸」页（武器、重铸、重铸范围）；
/// 4 = 加「超限」页（dr_ws_overlimit，快照 ostones / wovl / wocap / aovl / acut）；5 = 加「魇印」页（dr_ws_seal、快照 nm*、提示 get(3)）；
/// 6 = 加「七咒」页（快照 cur）；7 = 「魇印」「七咒」各自从赐福「暗黑重铸」一项打开（dr_ws_state 第 1~2 位 = 入口）；
/// 8 = 去掉提示 get(3)（改成原版闪烁提示，dark_core 的 notes.rs）；9 = 「超限」页防具词条升级（dr_ws_affix、快照 aup）；
/// 12 = 快照只有索引，明细按需要来（dr_ws_want、get(3)）；13 = 索引带获得顺序和词条键，want 加 s（搜索文字）；
/// 14 = 「超限」页武器栏的弃置按钮（dr_ws_discard，快照 w 行末尾多一列弃置 0|1）
pub const API: u32 = 14;

#[derive(Clone, Copy)]
pub struct Api {
    state: unsafe extern "C" fn() -> u32,
    close: unsafe extern "C" fn(),
    refresh: unsafe extern "C" fn(),
    apply: unsafe extern "C" fn(u32, u32, i64, i32),
    reforge: unsafe extern "C" fn(u32),
    preview: unsafe extern "C" fn(u32),
    overlimit: unsafe extern "C" fn(u32, u32, u32),
    seal: unsafe extern "C" fn(u32, i32),
    affix: unsafe extern "C" fn(u32, u32),
    discard: unsafe extern "C" fn(u32),
    get: unsafe extern "C" fn(u32, *mut u8, usize) -> i64,
    want: unsafe extern "C" fn(*const u8, usize),
}

impl Api {
    /// 找 dark_core.dll 导出的函数（没加载、版本太老就 None）
    pub fn find() -> Option<Api> {
        let m = unsafe { GetModuleHandleA(s!("dark_core.dll")) }.ok()?;
        unsafe {
            Some(Api {
                state: std::mem::transmute(GetProcAddress(m, s!("dr_ws_state"))?),
                close: std::mem::transmute(GetProcAddress(m, s!("dr_ws_close"))?),
                refresh: std::mem::transmute(GetProcAddress(m, s!("dr_ws_refresh"))?),
                apply: std::mem::transmute(GetProcAddress(m, s!("dr_ws_apply"))?),
                reforge: std::mem::transmute(GetProcAddress(m, s!("dr_ws_reforge"))?),
                preview: std::mem::transmute(GetProcAddress(m, s!("dr_ws_preview"))?),
                overlimit: std::mem::transmute(GetProcAddress(m, s!("dr_ws_overlimit"))?),
                seal: std::mem::transmute(GetProcAddress(m, s!("dr_ws_seal"))?),
                affix: std::mem::transmute(GetProcAddress(m, s!("dr_ws_affix"))?),
                discard: std::mem::transmute(GetProcAddress(m, s!("dr_ws_discard"))?),
                get: std::mem::transmute(GetProcAddress(m, s!("dr_ws_get"))?),
                want: std::mem::transmute(GetProcAddress(m, s!("dr_ws_want"))?),
            })
        }
    }

    /// 找不到时说明缺了什么（日志用）
    pub fn missing() -> String {
        let Ok(m) = (unsafe { GetModuleHandleA(s!("dark_core.dll")) }) else { return "dark_core.dll 没加载".into() };
        let names = [s!("dr_ws_state"), s!("dr_ws_close"), s!("dr_ws_refresh"), s!("dr_ws_apply"), s!("dr_ws_reforge"), s!("dr_ws_preview"), s!("dr_ws_overlimit"), s!("dr_ws_seal"), s!("dr_ws_affix"), s!("dr_ws_discard"), s!("dr_ws_get"), s!("dr_ws_want")];
        let miss: Vec<String> = names
            .iter()
            .filter(|n| unsafe { GetProcAddress(m, **n) }.is_none())
            .map(|n| unsafe { n.to_string() }.unwrap_or_default())
            .collect();
        if miss.is_empty() { "函数都在".into() } else { format!("dark_core 缺 {}（dark_core 和 dark_ui 不是同一版）", miss.join("、")) }
    }

    /// (接口版本, 窗口该不该开着)
    /// (接口版本, 窗口该不该开着, 入口：0 装备工坊、1 魇印、2 七咒)
    pub fn state(&self) -> (u32, bool, usize) {
        let v = unsafe { (self.state)() };
        (v >> 16, v & 1 != 0, ((v >> 1) & 3) as usize)
    }

    pub fn close(&self) {
        unsafe { (self.close)() }
    }

    pub fn refresh(&self) {
        unsafe { (self.refresh)() }
    }

    /// target None = 提取成护符；replace None = 补上
    pub fn apply(&self, src: u32, k: usize, target: Option<u32>, replace: Option<usize>) {
        unsafe { (self.apply)(src, k as u32, target.map_or(-1, |t| t as i64), replace.map_or(-1, |r| r as i32)) }
    }

    /// 重铸这把武器（配方序号）
    pub fn reforge(&self, k: u32) {
        unsafe { (self.reforge)(k) }
    }

    /// 超限：cat 0 武器 / 1 防具、配方序号、最多用几颗（u32::MAX = 用到上限）
    pub fn overlimit(&self, cat: u32, k: u32, n: u32) {
        unsafe { (self.overlimit)(cat, k, n) }
    }

    /// 防具词条升级：配方序号 k 的第 i 条升一档
    pub fn affix(&self, k: u32, i: usize) {
        unsafe { (self.affix)(k, i as u32) }
    }

    /// 弃置 / 取消弃置一把生成武器（配方序号）
    pub fn discard(&self, k: u32) {
        unsafe { (self.discard)(k) }
    }

    /// 魇印：把第 fam 种刻进第 slot 位（None = 取下）
    pub fn seal(&self, slot: usize, fam: Option<usize>) {
        unsafe { (self.seal)(slot as u32, fam.map_or(-1, |k| k as i32)) }
    }

    /// 要明细（格式见 dark_core 的 workshop.rs 文件头；算好了 get(3) 取）
    pub fn want(&self, t: &str) {
        unsafe { (self.want)(t.as_ptr(), t.len()) }
    }

    /// 要这把武器重铸后的范围（之后 get(2)）
    pub fn preview(&self, k: u32) {
        unsafe { (self.preview)(k) }
    }

    /// which 0 = 快照、1 = 结果、2 = 重铸范围、3 = 算好的明细（取走就清空）→（版本, 文字）；版本 0 = 还没有
    pub fn get(&self, which: u32) -> (u32, String) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let r = unsafe { (self.get)(which, buf.as_mut_ptr(), buf.len()) };
            if r < 0 {
                buf.resize((-r) as usize + 1024, 0);
                continue;
            }
            let len = (r & 0xffff_ffff) as usize;
            return ((r >> 32) as u32, String::from_utf8_lossy(&buf[..len.min(buf.len())]).into_owned());
        }
    }
}

/// 一件生成防具（icon：游戏里显示的图标，带稀有度边框的那份）。明细没来时 loaded = false，只有索引里的字段（序号、稀有度、部位、穿着、锁定）
#[derive(Debug, Clone, Default)]
pub struct Piece {
    pub idx: u32,
    pub rar: usize,
    pub part: usize,
    pub worn: bool,
    pub locked: bool,
    pub icon: u16,
    pub name: String,
    pub loaded: bool,
    /// 能当来源（没穿着、没锁定）、有超限数据
    pub is_src: bool,
    pub ovl: bool,
    /// 获得顺序（越大越新）、身上词条的键
    pub sort: u32,
    pub keys: Vec<String>,
    /// 减伤率（八种减伤的平均，%）、重量：游戏参数里这件现在的数，继承页排序用（索引 a 行最后两列，旧的 dark_core 没有 = 0）
    pub cut: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Aff {
    pub mech: bool,
    pub rar: usize,
    pub name: String,
    pub text: String,
}

/// 一种放法：替换目标的第 rep 条（-1 = 补上）、能不能、不能的原因
#[derive(Debug, Clone)]
pub struct Opt {
    pub rep: i32,
    pub ok: bool,
    pub why: String,
}

/// 背包里的一把生成武器和原版那样的数值（dark_core 的 weapon_info 算的）
#[derive(Debug, Clone, Default)]
pub struct Wpn {
    pub idx: u32,
    pub rar: usize,
    pub icon: u16,
    pub worn: bool,
    pub locked: bool,
    /// 被玩家标了弃置（「超限」页武器栏的第 4 个按钮；只影响一键出售和武器回收）
    pub discard: bool,
    pub level: u32,
    pub wtype: u16,
    pub name: String,
    /// 物理、魔力、火、雷、圣：（基础, 加成）
    pub atk: [(i32, i32); 5],
    pub crit: i32,
    pub guard: [f32; 5],
    pub boost: i32,
    /// 补正（当前强化等级，0 = 没有）、能力需求：力 灵 智 信 感
    pub cor: [i32; 5],
    pub req: [i32; 5],
    pub weight: f32,
    pub skill: String,
    pub fx: Vec<String>,
    /// 超限过（不能再重铸）、到超限上限还要几颗
    pub ovl: bool,
    pub need: u32,
    /// 超限：有补正的每项（现在, 中位, 原上限, 超限上限, 每颗），都换算成面板上的数
    pub ocap: [Option<[f32; 5]>; 5],
    /// 明细来了没有（没来时只有索引：序号、稀有度、装备着、锁定、wepType）、有超限数据（超限页列得出来）
    pub loaded: bool,
    pub ovl_able: bool,
    /// 获得顺序（越大越新）、特效的键（没有是空的）
    pub sort: u32,
    pub spec: String,
}

/// 魇印的刻印位数（同 rr/nightmare.py 的 SLOT_FLAGS）
pub const NM_SLOTS: usize = 5;

/// 魇印的一种：颜色 0~3、夺取次数、等级 0~4、下一级要几次（0 = 满级）、名字、现在这一级 / 下一级的说明
#[derive(Debug, Clone, Default)]
pub struct NmFam {
    pub idx: usize,
    pub color: usize,
    pub count: u32,
    pub level: usize,
    pub next: u32,
    pub name: String,
    pub now: String,
    pub up: String,
    /// 护符模板的图标（按等级的稀有度，带边框）
    pub icon: u16,
}

/// 七咒的一条：碎片、要几片、反转了没有、名字、区域、诅咒 / 祝福说明
#[derive(Debug, Clone, Default)]
pub struct CurseRow {
    pub shards: u32,
    pub need: u32,
    pub rev: bool,
    pub name: String,
    pub region: String,
    pub curse: String,
    pub bless: String,
    /// 来历（右栏最下面）
    pub lore: String,
    /// 现在这一面的图标（0 = 菜单包里没有）
    pub icon: u16,
}

/// 一件生成防具的超限数据：现在的减伤倍率、中位、原上限、超限上限、每颗、到上限还要几颗、超限过、本体原版八种减伤（无属性 斩 打 突 魔 火 雷 圣）
#[derive(Debug, Clone, Default)]
pub struct Aovl {
    pub m: f32,
    pub med: f32,
    pub hi: f32,
    pub cap: f32,
    pub step: f32,
    pub need: u32,
    pub ovl: bool,
    pub cut: [f32; 8],
}

/// 防具一条词条能不能升级：升到的稀有度、要几颗防具石、升级后的说明（不能升时是原因）
#[derive(Debug, Clone, Default)]
pub struct Aup {
    pub ok: bool,
    pub rar: usize,
    pub cost: u32,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Snap {
    pub lang: usize,
    /// 背包里的融锻之心（继承消耗的道具）和它的图标
    pub stones: u32,
    pub stone_icon: u16,
    /// 武器重铸石（重铸消耗的）和它的图标
    pub rstones: u32,
    pub rstone_icon: u16,
    /// 人物现在的属性：力 灵 智 信 感
    pub stats: [i32; 5],
    /// 真实游戏的快照只有索引（a / w 行），明细按需要来（merge）；离线预览的模拟快照是完整的（false）
    pub lazy: bool,
    /// 关键词搜索用的文字（小写）：(0 防具 / 1 武器, 序号) → 文字；发完了 search_done
    pub search: HashMap<(u8, u32), String>,
    pub search_done: bool,
    /// 全部生成防具（背包里的顺序）
    pub pieces: Vec<Piece>,
    ppos: HashMap<u32, usize>,
    /// 各个列表（pieces / weapons 里的位置，finish 算好，之后不变）：来源（全部 + 四个部位）、目标（穿着的在前，再按部位、稀有度）、
    /// 目标的名次（序号 → tgt_view 里第几个）、武器（全部 / 近战 / 远程与法器 / 盾）、超限页（武器 / 防具）
    pub src_view: [Vec<usize>; 5],
    pub tgt_view: Vec<usize>,
    pub tgt_rank: HashMap<u32, usize>,
    pub wpn_view: [Vec<usize>; 4],
    pub ovl_view: [Vec<usize>; 2],
    pub affs: HashMap<u32, Vec<Aff>>,
    /// (来源, 第 k 条, 目标) → 各种放法；整件放不了的在 nopt
    pub opts: HashMap<(u32, usize, u32), Vec<Opt>>,
    pub nopt: HashMap<(u32, usize, u32), String>,
    /// (来源, 第 k 条) → 提取成护符：能不能、护符图标、护符名字 / 原因
    pub tal: HashMap<(u32, usize), Tal>,
    pub weapons: Vec<Wpn>,
    wpos: HashMap<u32, usize>,
    /// 超限锻造石（武器, 防具）和图标；各件生成防具的超限数据（按配方序号）
    pub ostones: [u32; 2],
    pub ostone_icon: [u16; 2],
    pub aovl: HashMap<u32, Aovl>,
    /// (配方序号, 第 i 条) → 词条升级
    pub aup: HashMap<(u32, usize), Aup>,
    /// 魇印：开了几个刻印位、各刻印位刻的第几种、各种、身上的魇火 (第几种, 稀有度)
    pub nm_open: usize,
    pub nm_slots: [Option<usize>; NM_SLOTS],
    /// 各刻印位要解锁几种
    pub nm_need: Vec<u32>,
    pub nm_fams: Vec<NmFam>,
    pub nm_embers: Vec<(usize, usize, u16)>,
    /// 七咒（7 条）
    pub curses: Vec<CurseRow>,
    /// 七咒碎片的图标：诅咒、祝福、没拿到（0 = 菜单包里没有）
    pub shard_icons: [u16; 3],
}

#[derive(Debug, Clone)]
pub struct Tal {
    pub ok: bool,
    pub icon: u16,
    pub text: String,
}

/// 重铸后的范围（dark_core 按现在的加点抽很多次统计的）
#[derive(Debug, Clone, Default)]
pub struct Prev {
    pub idx: u32,
    /// 有 1..5 项补正的百分比
    pub n: [f32; 5],
    /// 每项属性：（有补正的百分比, 补正最小, 最大, 需求最小, 最大）
    pub s: [(f32, i32, i32, i32, i32); 5],
}

impl Prev {
    pub fn parse(text: &str) -> Option<Prev> {
        let mut p = Prev::default();
        let mut seen = false;
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let fl = |i: usize| f.get(i).and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
            match f[0] {
                "pv" => {
                    p.idx = fl(1) as u32;
                    seen = true;
                }
                "pvn" => p.n = std::array::from_fn(|i| fl(i + 1)),
                "pvs" if f.len() >= 7 => {
                    let s = (fl(1) as usize).min(4);
                    p.s[s] = (fl(2), fl(3) as i32, fl(4) as i32, fl(5) as i32, fl(6) as i32);
                }
                _ => {}
            }
        }
        seen.then_some(p)
    }
}

fn unesc(s: &str) -> String {
    s.replace("\\n", "\n")
}

/// 武器类别（重铸页的页签）：1 近战、2 弓弩和法器、3 盾
fn wcat(t: u16) -> usize {
    match t {
        50..=61 => 2,
        65 | 67 | 69 | 90 => 3,
        _ => 1,
    }
}

impl Snap {
    /// 一份快照（真实游戏是索引，离线预览是完整数据）
    pub fn parse(text: &str) -> Snap {
        let mut d = Snap::default();
        d.merge(text);
        d.finish();
        d
    }

    /// 这件防具的行：索引里没有的，离线预览的完整快照新建，真实游戏不要
    fn piece_slot(&mut self, idx: u32) -> Option<&mut Piece> {
        if !self.ppos.contains_key(&idx) {
            if self.lazy {
                return None;
            }
            self.ppos.insert(idx, self.pieces.len());
            self.pieces.push(Piece { idx, ..Piece::default() });
        }
        let i = self.ppos[&idx];
        self.pieces.get_mut(i)
    }

    /// 列表（只看索引里的字段，明细来了也不变）
    fn finish(&mut self) {
        for p in &mut self.pieces {
            p.ovl |= self.aovl.contains_key(&p.idx);
        }
        for w in &mut self.weapons {
            w.ovl_able |= w.ocap.iter().any(Option::is_some);
        }
        let (p, w) = (&self.pieces, &self.weapons);
        // 都是装备中的排最前面，然后按获得顺序（新的在前）
        self.src_view = std::array::from_fn(|f| {
            let mut v: Vec<usize> = (0..p.len()).filter(|&i| p[i].is_src && (f == 0 || p[i].part + 1 == f)).collect();
            v.sort_by_key(|&i| (!p[i].worn, std::cmp::Reverse(p[i].sort)));
            v
        });
        let mut t: Vec<usize> = (0..p.len()).collect();
        t.sort_by_key(|&i| (!p[i].worn, std::cmp::Reverse(p[i].sort)));
        self.tgt_rank = t.iter().enumerate().map(|(r, &i)| (p[i].idx, r)).collect();
        self.tgt_view = t;
        self.wpn_view = std::array::from_fn(|f| {
            let mut v: Vec<usize> = (0..w.len()).filter(|&i| f == 0 || wcat(w[i].wtype) == f).collect();
            v.sort_by_key(|&i| (!w[i].worn, std::cmp::Reverse(w[i].sort)));
            v
        });
        let mut ow: Vec<usize> = (0..w.len()).filter(|&i| w[i].ovl_able).collect();
        ow.sort_by_key(|&i| (!w[i].worn, std::cmp::Reverse(w[i].sort)));
        let mut oa: Vec<usize> = (0..p.len()).filter(|&i| p[i].ovl).collect();
        oa.sort_by_key(|&i| (!p[i].worn, std::cmp::Reverse(p[i].sort)));
        self.ovl_view = [ow, oa];
    }

    /// 合进一段文字：快照，或者 dr_ws_get(3) 的明细
    pub fn merge(&mut self, text: &str) {
        let d = self;
        // 这一段里第一次见到的放法先清掉旧的（同一组可能要过两次）
        let mut seen: HashSet<(u32, usize, u32)> = HashSet::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|x| x.parse::<i64>().ok()).unwrap_or(-1);
            let fl = |i: usize| f.get(i).and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
            let s = |i: usize| f.get(i).map(|x| unesc(x)).unwrap_or_default();
            let icon = |i: usize| n(i).clamp(0, u16::MAX as i64) as u16;
            let wpn = |d: &Snap| -> Option<usize> { d.wpos.get(&(n(1) as u32)).copied() };
            let keys = |i: usize| -> Vec<String> { f.get(i).filter(|x| **x != "-").map(|x| x.split(',').map(String::from).collect()).unwrap_or_default() };
            match f[0] {
                "lang" => d.lang = n(1).clamp(0, 1) as usize,
                "stones" => d.stones = n(1).max(0) as u32,
                "stone_icon" => d.stone_icon = icon(1),
                "rstones" => d.rstones = n(1).max(0) as u32,
                "rstone_icon" => d.rstone_icon = icon(1),
                "pst" => d.stats = std::array::from_fn(|i| n(i + 1) as i32),
                "a" if f.len() >= 9 => {
                    d.lazy = true;
                    d.ppos.insert(n(1) as u32, d.pieces.len());
                    let (worn, locked) = (n(4) == 1, n(5) == 1);
                    d.pieces.push(Piece {
                        idx: n(1) as u32,
                        rar: n(2).clamp(0, 3) as usize,
                        part: n(3).clamp(0, 9) as usize,
                        worn,
                        locked,
                        is_src: !worn && !locked,
                        ovl: n(6) == 1,
                        sort: n(7).max(0) as u32,
                        keys: keys(8),
                        cut: fl(9),
                        weight: fl(10),
                        ..Piece::default()
                    });
                }
                "w" if f.len() >= 9 => {
                    d.lazy = true;
                    d.wpos.insert(n(1) as u32, d.weapons.len());
                    d.weapons.push(Wpn { idx: n(1) as u32, rar: n(2).clamp(0, 3) as usize, worn: n(3) == 1, locked: n(4) == 1, wtype: n(5).clamp(0, u16::MAX as i64) as u16, ovl_able: n(6) == 1, sort: n(7).max(0) as u32, spec: keys(8).pop().unwrap_or_default(), discard: f.get(9).is_some_and(|x| *x == "1"), ..Wpn::default() });
                }
                "sa" if f.len() >= 3 => _ = d.search.insert((0, n(1) as u32), s(2).to_lowercase()),
                "sw" if f.len() >= 3 => _ = d.search.insert((1, n(1) as u32), s(2).to_lowercase()),
                "sdone" => d.search_done = true,
                // 离线预览的完整快照才有 src、pk、wk 行（pk / wk：获得顺序和词条键，真实游戏在索引的 a / w 行里）
                "pk" if f.len() >= 4 => {
                    if let Some(p) = d.piece_slot(n(1) as u32) {
                        (p.sort, p.keys) = (n(2).max(0) as u32, keys(3));
                    }
                }
                "wk" if f.len() >= 4 => {
                    if let Some(i) = wpn(d) {
                        (d.weapons[i].sort, d.weapons[i].spec) = (n(2).max(0) as u32, keys(3).pop().unwrap_or_default());
                    }
                }
                "src" if f.len() >= 6 => {
                    if let Some(p) = d.piece_slot(n(1) as u32) {
                        (p.rar, p.part, p.icon, p.name, p.loaded, p.is_src) = (n(2).clamp(0, 3) as usize, n(3).clamp(0, 9) as usize, icon(4), s(5), true, true);
                    }
                }
                "tgt" if f.len() >= 8 => {
                    let idx = n(1) as u32;
                    if let Some(p) = d.piece_slot(idx) {
                        (p.rar, p.part, p.worn, p.locked, p.icon, p.name, p.loaded) = (n(2).clamp(0, 3) as usize, n(3).clamp(0, 9) as usize, n(4) == 1, n(5) == 1, icon(6), s(7), true);
                        if d.lazy {
                            d.affs.remove(&idx); // 后面跟着这件的词条
                        }
                    }
                }
                "aff" if f.len() >= 7 => d.affs.entry(n(1) as u32).or_default().push(Aff { mech: n(3) == 1, rar: n(4).clamp(0, 3) as usize, name: s(5), text: s(6) }),
                "opt" if f.len() >= 6 => {
                    let key = (n(1) as u32, n(2) as usize, n(3) as u32);
                    if seen.insert(key) {
                        d.opts.remove(&key);
                    }
                    d.opts.entry(key).or_default().push(Opt { rep: n(4) as i32, ok: n(5) == 1, why: s(6) });
                }
                "nopt" if f.len() >= 5 => _ = d.nopt.insert((n(1) as u32, n(2) as usize, n(3) as u32), s(4)),
                "tal" if f.len() >= 6 => _ = d.tal.insert((n(1) as u32, n(2) as usize), Tal { ok: n(3) == 1, icon: icon(4), text: s(5) }),
                "wpn" if f.len() >= 9 => {
                    let idx = n(1) as u32;
                    let w = Wpn {
                        idx,
                        rar: n(2).clamp(0, 3) as usize,
                        icon: icon(3),
                        worn: n(4) == 1,
                        locked: n(5) == 1,
                        level: n(6).max(0) as u32,
                        wtype: n(7).clamp(0, u16::MAX as i64) as u16,
                        name: s(8),
                        loaded: true,
                        ..Wpn::default()
                    };
                    match d.wpos.get(&idx).copied() {
                        Some(i) => {
                            // discard 只在索引行里（w 行第 9 列），明细 wpn 行没有这一列，覆盖时要留住
                            let (ovl_able, sort, spec, discard) = (d.weapons[i].ovl_able, d.weapons[i].sort, d.weapons[i].spec.clone(), d.weapons[i].discard);
                            d.weapons[i] = Wpn { ovl_able, sort, spec, discard, ..w };
                        }
                        None if !d.lazy => {
                            d.wpos.insert(idx, d.weapons.len());
                            d.weapons.push(w);
                        }
                        None => {}
                    }
                }
                "watk" if f.len() >= 13 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].atk = std::array::from_fn(|e| (n(2 + e * 2) as i32, n(3 + e * 2) as i32));
                        d.weapons[i].crit = n(12) as i32;
                    }
                }
                "wgrd" if f.len() >= 8 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].guard = std::array::from_fn(|e| fl(2 + e));
                        d.weapons[i].boost = n(7) as i32;
                    }
                }
                "wcor" if f.len() >= 7 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].cor = std::array::from_fn(|k| n(2 + k) as i32);
                    }
                }
                "wreq" if f.len() >= 7 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].req = std::array::from_fn(|k| n(2 + k) as i32);
                    }
                }
                "winf" if f.len() >= 3 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].weight = fl(2);
                        d.weapons[i].skill = s(3);
                    }
                }
                "wfx" if f.len() >= 3 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].fx.push(s(2));
                    }
                }
                "ostones" if f.len() >= 3 => d.ostones = [n(1).max(0) as u32, n(2).max(0) as u32],
                "ostone_icon" if f.len() >= 3 => d.ostone_icon = [icon(1), icon(2)],
                "cshard" if f.len() >= 4 => d.shard_icons = [icon(1), icon(2), icon(3)],
                "wovl" if f.len() >= 4 => {
                    if let Some(i) = wpn(d) {
                        d.weapons[i].ovl = n(2) == 1;
                        d.weapons[i].need = n(3).max(0) as u32;
                    }
                }
                "wocap" if f.len() >= 8 => {
                    if let Some(i) = wpn(d) {
                        let k = n(2).clamp(0, 4) as usize;
                        d.weapons[i].ocap[k] = Some(std::array::from_fn(|j| fl(3 + j)));
                    }
                }
                "aovl" if f.len() >= 9 => {
                    let a = d.aovl.entry(n(1) as u32).or_default();
                    (a.m, a.med, a.hi, a.cap, a.step) = (fl(2), fl(3), fl(4), fl(5), fl(6));
                    (a.need, a.ovl) = (n(7).max(0) as u32, n(8) == 1);
                }
                "cur" if f.len() >= 11 => d.curses.push(CurseRow {
                    shards: n(2).max(0) as u32,
                    need: n(3).max(0) as u32, // 0 = 没有碎片（对勇于挑战者的馈赠）
                    rev: n(4) == 1,
                    name: s(5),
                    region: s(6),
                    curse: s(7),
                    bless: s(8),
                    lore: s(9),
                    icon: n(10).max(0) as u16,
                }),
                "nmo" if f.len() >= 2 => {
                    d.nm_open = n(1).clamp(0, NM_SLOTS as i64) as usize;
                    d.nm_need = s(3).split(',').filter_map(|x| x.parse().ok()).collect();
                }
                "nms" if f.len() >= 3 => {
                    let i = n(1).clamp(0, NM_SLOTS as i64 - 1) as usize;
                    d.nm_slots[i] = usize::try_from(n(2)).ok();
                }
                "nme" if f.len() >= 3 => d.nm_embers.push((n(1).max(0) as usize, n(2).clamp(0, 3) as usize, icon(3))),
                "nmf" if f.len() >= 9 => d.nm_fams.push(NmFam {
                    idx: n(1).max(0) as usize,
                    color: n(2).clamp(0, 3) as usize,
                    count: n(3).max(0) as u32,
                    level: n(4).clamp(0, 4) as usize,
                    next: n(5).max(0) as u32,
                    name: s(6),
                    now: s(7),
                    up: s(8),
                    icon: icon(9),
                }),
                "aup" if f.len() >= 7 => _ = d.aup.insert(
                    (n(1) as u32, n(2).max(0) as usize),
                    Aup { ok: n(3) == 1, rar: n(4).clamp(0, 3) as usize, cost: n(5).max(0) as u32, text: s(6) },
                ),
                "acut" if f.len() >= 10 => d.aovl.entry(n(1) as u32).or_default().cut = std::array::from_fn(|j| fl(2 + j)),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 索引行（w 行第 9 列 = 弃置）先来、明细行（wpn，没有弃置列）后到，
    /// 覆盖时不能把弃置标记抹掉——2026-11-26 实测红叉一个都不显示就是这个原因
    #[test]
    fn discard_survives_detail_row() {
        let s = Snap::parse("w\t1\t3\t0\t0\t0\t1\t5\t-\t1\nwpn\t1\t3\t120\t0\t0\t0\t0\t测试剑\n");
        assert!(s.weapons[0].loaded, "明细行应该覆盖进来了");
        assert!(s.weapons[0].discard, "明细行覆盖时把索引行的弃置标记丢了");
        assert_eq!(s.weapons[0].name, "测试剑");
    }

    /// 老格式（没有第 9 列）也要能读，按未弃置算
    #[test]
    fn discard_absent_means_false() {
        let s = Snap::parse("w\t1\t3\t0\t0\t0\t1\t5\t-\n");
        assert!(!s.weapons[0].discard);
    }
}
