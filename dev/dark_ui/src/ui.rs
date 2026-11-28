//! 装备工坊的界面（用户 2026-10-31：照原版商店的大小和素材）：一屏四块——
//!   左：来源列表（背包里没穿着、没锁定的生成防具），一行一件、带图标，像商店的货架；上面一排部位页签（X / Tab 切换）
//!   中上：左边选中那件的名字（原版的稀有度名牌底）和它的词条，选一条
//!   右：目标格子（背包里的生成防具，穿着的在前；第一格是「提取成护符」），放不了的变暗
//!   中下：右边选好以后，目标的各个位置（前缀 / 后缀 / 空位）：哪个能替换、能补上，不能的写原因；下面是之后的样子和消耗
//! 操作：手柄 十字键 / 左摇杆移动，A 下一步（最后一步弹确认），B 上一步（左栏里关窗口），LB / RB 换栏，X 部位，Y 直接确认；
//! 键盘 方向键、Enter、Esc / Backspace、Tab，R = Y；
//! 防误触（用户 2026-11-19：玩家按住回车一进工坊就一直自动继承）：键盘只有方向键按住连发，其他键按一下算一下（同手柄）；
//! 开窗口时还按着的键（选菜单的确认键）松开以后才算；确认框弹出 CONFIRM_GUARD 秒内不收「确定」（确定字样先暗着）。
//! 鼠标（用户 2026-11-09「做一下对鼠标的兼容」）：停在当前栏的一行上就选中（原版的做法），左键 = A（点别的栏先选上），右键 = B，滚轮翻列表（选中的行不动），
//! 滚动条能拖，底部按键提示能点；手柄、键盘一动列表又跟着选中的行。
//! 数据按需加载（同天「不要一次性把数据都加载完」）：快照只有索引，看得见的行 ± 余量的明细用 Req::Want 要（request_data），图标同样多预备一点（Draw::preload）。
//! 画法：全屏一个没有边框的窗口，按 1920×1080 的设计坐标摆（等比缩放、居中），图片、文字都直接画进窗口的绘制列表；
//! 图（原版素材、物品图标）从 Tex 要，游戏里第一次要的时候还没准备好就先不画（lib.rs 下一帧传上去）。
//! 这里不碰游戏、不碰 dark_core：要做的事放进 reqs（lib.rs 照着调 dark_core 的接口）。
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use hudhook::imgui::{FontId, ImColor32, Key, MouseButton, TextureId, Ui};

use crate::data::{Aff, CurseRow, NmFam, Opt, Piece, Prev, Snap, Wpn, NM_SLOTS};

mod codex;
mod curse;
mod filter;
mod overlimit;
mod pinyin;
mod reforge;
mod seal;

pub use codex::Codex;
use filter::{Picker, ViewKey};

/// 一张已经能画的图
#[derive(Clone, Copy)]
pub struct Img {
    pub id: TextureId,
    pub w: f32,
    pub h: f32,
}

pub trait Tex {
    /// 原版素材（menu.py 导出的名字，比如 MENU_FL_Cursor_Wide）
    fn sprite(&mut self, name: &str) -> Option<Img>;
    /// 物品图标（游戏参数里的图标编号）
    fn icon(&mut self, id: u16) -> Option<Img>;
    /// 马上要滚进来的图标：提前解好（排在画面上要的后面）
    fn preload_icon(&mut self, _id: u16) {}
}

/// imgui 的 FontId 里是裸指针（不能跨线程）；界面只在渲染线程用它
#[derive(Clone, Copy)]
pub struct UiFont(pub FontId);
unsafe impl Send for UiFont {}
unsafe impl Sync for UiFont {}

/// 要 lib.rs 去做的
#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    Close,
    Apply { src: u32, k: usize, target: Option<u32>, replace: Option<usize> },
    /// 重铸这把武器（配方序号）
    Reforge(u32),
    /// 要这把武器重铸后的范围
    Preview(u32),
    /// 超限：cat 0 武器 / 1 防具、配方序号、用几颗
    Overlimit { cat: u32, k: u32, n: u32 },
    /// 弃置 / 取消弃置一把生成武器（「超限」页武器栏第 4 个按钮，配方序号）
    Discard(u32),
    /// 魇印：把第 fam 种刻进第 slot 位（None = 取下）
    Seal { slot: usize, fam: Option<usize> },
    /// 防具词条升级：配方序号 k 的第 i 条
    Affix { k: u32, i: usize },
    /// 要明细（格式见 dark_core 的 workshop.rs 文件头）
    Want(String),
}

/// 字体按这个像素大小建（文字按需要缩放）
pub const FONT_PX: f32 = 32.0;
/// 字的墨迹中线在画的位置往下多少（× 字号）：用户 2026-11-04「带图标的文字需要下移到图标对称轴上 现在整体都偏高」。
/// 离线预览（out\menu\dark_ui_sprites.bin + 游戏字体，和游戏里同一套）量：中文字墨迹中心在 y + 0.40~0.43 × 字号；
/// 用它回算用户截图（游戏约 0.83 倍显示）里的偏高量（底部提示 3、类别 4、「用 1 颗」2.5 像素）也对得上
const TEXT_MID: f32 = 0.42;

// 手柄按键（XINPUT_GAMEPAD_*）；左摇杆推到底也算十字键（lib.rs 合进来）
pub const P_UP: u16 = 0x0001;
pub const P_START: u16 = 0x0010;
pub const P_DOWN: u16 = 0x0002;
pub const P_LEFT: u16 = 0x0004;
pub const P_RIGHT: u16 = 0x0008;
pub const P_LB: u16 = 0x0100;
pub const P_RB: u16 = 0x0200;
pub const P_A: u16 = 0x1000;
pub const P_B: u16 = 0x2000;
pub const P_X: u16 = 0x4000;
pub const P_Y: u16 = 0x8000;
/// 扳机（XInput 里是模拟量，lib.rs 按到一半以上算按下，借 XInput 没用的两位）
pub const P_LT: u16 = 0x0400;
pub const P_RT: u16 = 0x0800;

/// 这一帧的操作
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Act {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub lb: bool,
    pub rb: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    /// 换页（继承 / 重铸 / 超限）
    pub lt: bool,
    pub rt: bool,
    /// 打开词条筛选（手柄 Start / 键盘 F）、开始输入关键词（/ 或 Ctrl+F）
    pub filter: bool,
    pub search: bool,
}

const TEXT: [u8; 3] = [232, 228, 216];
const DIM: [u8; 3] = [140, 135, 122];
const GOLD: [u8; 3] = [201, 181, 127];
const OKC: [u8; 3] = [156, 214, 140];
const ERRC: [u8; 3] = [224, 120, 96];
/// 稀有度的字色（照稀有度名字的颜色；史诗那个紫在深底上太暗，提亮一点）
const RAR: [[u8; 3]; 4] = [[222, 222, 222], [160, 180, 243], [206, 128, 216], [226, 188, 64]];
const RAR_NAME: [[&str; 4]; 2] = [["普通", "稀有", "史诗", "传说"], ["Common", "Rare", "Epic", "Legendary"]];
const PART: [[&str; 4]; 2] = [["头部", "身体", "手部", "腿部"], ["Head", "Chest", "Arms", "Legs"]];
const FILTER: [[&str; 5]; 2] = [["全部", "头部", "身体", "手部", "腿部"], ["All", "Head", "Chest", "Arms", "Legs"]];
const TABS: [&str; 5] = ["MENU_Tab_Armor", "MENU_Tab_10", "MENU_Tab_11", "MENU_Tab_12", "MENU_Tab_13"];

fn c(rgb: [u8; 3], a: f32) -> ImColor32 {
    ImColor32::from_rgba(rgb[0], rgb[1], rgb[2], (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// 右边的一格
#[derive(Clone, Debug)]
enum Cell {
    Tal { ok: bool, icon: u16, text: String },
    /// known = 这一组放法已经加载了
    Tgt { p: Piece, ok: bool, why: String, known: bool },
}

impl Cell {
    fn ok(&self) -> bool {
        match self {
            Cell::Tal { ok, .. } | Cell::Tgt { ok, .. } => *ok,
        }
    }
}

/// 中下的一行：目标的一个位置（或者提取成护符）
#[derive(Clone, Debug)]
struct Slot {
    label: String,
    aff: Option<Aff>,
    /// 这个位置能做的：替换第 rep 条 / 补上（-1）/ 提取成护符（-3）；ok = 能，why = 不能的原因
    act: Option<(i32, bool, String)>,
}

/// 能滚的列表
#[derive(Clone, Copy, PartialEq, Debug)]
enum Scroll {
    Src,
    Cell,
    Wpn,
    OItem,
    KRow,
    KRel,
    Pick,
}

/// 要过的明细（换快照前不再要）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Ask {
    A(u32),
    W(u32),
    O(u32, usize, u32),
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    /// 一栏的空白处（滚轮）、滚动条（拖）、底部的按键提示（点了 = 按那个键）
    Area(Scroll),
    Bar(Scroll),
    Guide(Act),
    /// 词条筛选：窗口里的一行、标题栏的按钮、左栏下面的清除；搜索框的清除
    PRow(usize),
    KfBtn,
    KfClear,
    QClear,
    /// 继承页的排序：下拉按钮、升序 / 降序按钮、下拉菜单里的一行
    SortBtn,
    SortOrd,
    SortRow(usize),
    /// 标题旁的页签（继承 / 重铸 / 超限）
    Page(usize),
    /// 超限页：武器 / 防具页签、左边一行、右边的按钮（用 1 颗 / 10 颗 / 到上限）
    OTab(usize),
    OItem(usize),
    OBtn(usize),
    /// 魇印页：左边一种、右边一个刻印位
    NFam(usize),
    NSlot(usize),
    /// 七咒页：左边一条
    CRow(usize),
    /// 词条百科：类别页签、左边一行、右边一条相关词条
    KTab(usize),
    KRow(usize),
    KRel(usize),
    /// 重铸页：左边一行武器、部位页签、重铸按钮、快速重铸按钮
    Wpn(usize),
    WTab(usize),
    Forge,
    QuickForge,
    Tab(usize),
    Src(usize),
    Aff(usize),
    Cell(usize),
    Slot(usize),
    Ok,
    Cancel,
}

pub struct Workshop {
    pub snap: Option<Snap>,
    /// 上一次执行的结果（成功, 文字）
    pub msg: Option<(bool, String)>,
    /// 等 dark_core 回结果
    pub waiting: bool,
    /// 当前栏：0 来源、1 词条、2 目标、3 位置
    pub col: usize,
    pub sel: [usize; 4],
    pub filter: usize,
    pub confirm: bool,
    /// 确认框弹出了几秒（frame 里累加；不到 CONFIRM_GUARD 不收「确定」）
    confirm_t: f32,
    /// 角落里的提示（素材没读到之类）
    pub note: String,
    pub reqs: Vec<Req>,
    /// 中文 / 英文界面用的字体（fonts.rs：游戏自带的方正书宋 / Agmena）
    pub fonts: [Option<UiFont>; 2],
    /// 页：0 继承、1 重铸、2 超限、3 魇印、4 七咒、5 词条百科
    pub page: usize,
    /// 打开的是哪个入口（赐福「暗黑重铸」里各自一项，用户 2026-11-03）：0 装备工坊（继承 / 重铸 / 超限）、1 魇印、2 七咒
    pub mode: usize,
    /// 重铸页：栏（0 武器列表、1 重铸按钮）、选中的武器（筛选后的序号）、类别筛选、列表第一行
    rcol: usize,
    rsel: usize,
    rfilter: usize,
    rtop: usize,
    /// 重铸范围（dark_core 算好的）、已经要过范围的武器
    pub preview: Option<Prev>,
    prev_for: Option<u32>,
    /// 超限页：栏（0 列表、1 按钮）、选中的（列表里的序号）、页签（0 武器 / 1 防具）、列表第一行、选中的按钮
    ocol: usize,
    osel: usize,
    otab: usize,
    otop: usize,
    obtn: usize,
    /// 魇印页：栏（0 词条、1 刻印位）、选中的词条、选中的刻印位
    ncol: usize,
    nsel: usize,
    nslot: usize,
    /// 七咒页：选中的那条
    csel: usize,
    /// 词条百科的内容（lib.rs 读词条库解析好的）
    pub codex: Codex,
    /// 词条百科页：栏（0 词条、1 相关词条）、类别、各类别选中的行和第一行、相关词条选中的行和第一行、跳转前的位置（B 回去）
    kcol: usize,
    kcat: usize,
    ksel: [usize; codex::CATS],
    ktop: [usize; codex::CATS],
    krel: usize,
    krtop: usize,
    khist: Vec<(usize, usize)>,
    top: [usize; 4],
    hits: Vec<([f32; 4], Hit)>,
    /// 列表跟着选中的行（手柄 / 键盘动过、点过）；滚轮、拖滚动条时不跟
    follow: bool,
    /// 鼠标停在哪、停了多久、指针动过没有（没动过不算：手柄翻页时列表从不动的指针下面滚过）
    hover: Option<Hit>,
    hover_t: f32,
    hover_moved: bool,
    /// 正在拖的滚动条、这一帧画的滚动条（列表, y, 高, 一屏几行, 总行数）
    drag: Option<Scroll>,
    bars: Vec<(Scroll, f32, f32, usize, usize)>,
    /// 要过的明细；刚要过明细之后的几帧（lib.rs 每帧取结果）；搜索文字要过了
    asked: HashSet<Ask>,
    loading: u32,
    search_asked: bool,
    /// 各页的关键词（序号 = page）；词条筛选（继承 / 重铸 / 超限武器 / 超限防具）；词条筛选窗口
    query: [String; 6],
    kf: [Vec<String>; 4],
    picker: Option<Picker>,
    /// 正在输入（键盘不当操作）、下一帧把焦点给输入框；手柄 / 方向键的光标在标题栏上（head_at：继承页 0 排序、1 升降序、2 词条筛选；别的页只有筛选）
    typing: bool,
    focus_search: bool,
    head: bool,
    head_at: usize,
    /// 继承页的排序（filter.rs）：0 默认（穿着的在前、按获得时间）、1 减伤率、2 重量；升序；下拉菜单开着（光标在第几行）
    sort: usize,
    asc: bool,
    sort_menu: Option<usize>,
    /// 继承页右边的目标（排好序的 pieces 位置）和名次（序号 → 第几个；来源自己不放进格子）
    v_tgt: Vec<usize>,
    tgt_rank: HashMap<u32, usize>,
    /// 筛选后的列表（pieces / weapons / 词条的位置）和算它们用的条件（filter.rs 的 refresh_views）；快照的代数
    v_src: Vec<usize>,
    v_wpn: Vec<usize>,
    v_ovl: Vec<usize>,
    v_k: Vec<usize>,
    view_key: Option<ViewKey>,
    snap_gen: u64,
    // 设计坐标 → 屏幕：缩放和左上角
    s: f32,
    o: [f32; 2],
}

impl Default for Workshop {
    fn default() -> Self {
        Workshop {
            snap: None,
            msg: None,
            waiting: false,
            col: 0,
            sel: [0; 4],
            filter: 0,
            confirm: false,
            confirm_t: 0.0,
            note: String::new(),
            reqs: Vec::new(),
            fonts: [None, None],
            page: 0,
            mode: 0,
            rcol: 0,
            rsel: 0,
            rfilter: 0,
            rtop: 0,
            preview: None,
            prev_for: None,
            ocol: 0,
            osel: 0,
            otab: 0,
            otop: 0,
            obtn: 0,
            ncol: 0,
            nsel: 0,
            nslot: 0,
            csel: 0,
            codex: Codex::default(),
            kcol: 0,
            kcat: 0,
            ksel: [0; codex::CATS],
            ktop: [0; codex::CATS],
            krel: 0,
            krtop: 0,
            khist: Vec::new(),
            top: [0; 4],
            hits: Vec::new(),
            follow: true,
            hover: None,
            hover_t: 0.0,
            hover_moved: false,
            drag: None,
            bars: Vec::new(),
            asked: HashSet::new(),
            loading: 0,
            search_asked: false,
            query: Default::default(),
            kf: Default::default(),
            picker: None,
            typing: false,
            focus_search: false,
            head: false,
            head_at: 2,
            sort: 0,
            asc: false,
            sort_menu: None,
            v_tgt: Vec::new(),
            tgt_rank: HashMap::new(),
            v_src: Vec::new(),
            v_wpn: Vec::new(),
            v_ovl: Vec::new(),
            v_k: Vec::new(),
            view_key: None,
            snap_gen: 0,
            s: 1.0,
            o: [0.0, 0.0],
        }
    }
}

impl Workshop {
    /// 重新打开窗口
    pub fn reset(&mut self) {
        *self = Workshop { snap: self.snap.take(), note: std::mem::take(&mut self.note), fonts: self.fonts, page: self.page, mode: self.mode, codex: std::mem::take(&mut self.codex), ..Workshop::default() };
    }

    /// 这个入口有哪几页
    fn pages(&self) -> &'static [usize] {
        match self.mode {
            1 => &[3],
            2 => &[4],
            _ => &[5, 0, 1, 2],
        }
    }

    /// 打开窗口时换入口：页不属于这个入口就停在它的第一页
    pub fn set_mode(&mut self, mode: usize) {
        self.mode = mode.min(2);
        if !self.pages().contains(&self.page) {
            self.page = self.pages()[0];
        }
    }

    pub fn lang(&self) -> usize {
        self.snap.as_ref().map_or(0, |s| s.lang)
    }

    fn t<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        if self.lang() == 0 { zh } else { en }
    }

    // ------------------------------------------------------------ 数据
    /// 换了快照：之前要过的都作废
    pub fn set_snap(&mut self, snap: Snap) {
        self.snap = Some(snap);
        self.asked.clear();
        self.search_asked = false;
        self.snap_gen += 1;
        self.refresh_views();
    }

    /// dark_core 算好的明细
    pub fn merge_patch(&mut self, text: &str) {
        if let Some(s) = &mut self.snap {
            s.merge(text);
        }
        // 搜索文字来了列表要重算
        if text.lines().any(|l| l.starts_with("sa\t") || l.starts_with("sw\t") || l == "sdone") {
            self.snap_gen += 1;
        }
    }

    /// 刚要过明细（lib.rs 这几帧每帧都取结果）
    pub fn loading(&self) -> bool {
        self.loading > 0 || (self.search_asked && !self.snap.as_ref().is_some_and(|s| s.search_done))
    }

    /// 这一帧用得到的明细（看得见的行，前后各留一点余量）里还没要过的，一次要齐。滚动时余量里的行先到，滚进来就已经有了
    fn request_data(&mut self) {
        self.refresh_views();
        if self.waiting || !self.snap.as_ref().is_some_and(|s| s.lazy) {
            return;
        }
        // 第一次搜索才要搜索文字
        if !self.search_asked && self.page <= 2 && !self.terms(self.page).is_empty() {
            self.search_asked = true;
            self.reqs.push(Req::Want("s\n".to_string()));
            self.loading = 30;
        }
        let asks = self.asks();
        let Some(s) = &self.snap else { return };
        let (mut a, mut w) = (Vec::new(), Vec::new());
        let mut o: BTreeMap<(u32, usize), Vec<u32>> = BTreeMap::new();
        for q in asks {
            if self.asked.contains(&q) {
                continue;
            }
            match q {
                Ask::A(i) if s.pieces.iter().any(|p| p.idx == i && !p.loaded) => a.push(i),
                Ask::W(i) if s.weapons.iter().any(|x| x.idx == i && !x.loaded) => w.push(i),
                Ask::O(x, k, t) if !s.opts.contains_key(&(x, k, t)) && !s.nopt.contains_key(&(x, k, t)) => o.entry((x, k)).or_default().push(t),
                _ => continue,
            }
            self.asked.insert(q);
        }
        let csv = |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let mut text = String::new();
        if !a.is_empty() {
            text += &format!("a {}\n", csv(&a));
        }
        if !w.is_empty() {
            text += &format!("w {}\n", csv(&w));
        }
        for ((x, k), t) in &o {
            text += &format!("o {x} {k} {}\n", csv(t));
        }
        if !text.is_empty() {
            self.reqs.push(Req::Want(text));
            self.loading = 30;
        }
    }

    /// 这一页现在用得到的明细
    fn asks(&self) -> Vec<Ask> {
        let Some(s) = &self.snap else { return Vec::new() };
        match self.page {
            0 => self.asks_inherit(),
            1 => near(self.wview(), self.rtop, LIST_VIS, LIST_MARGIN, self.rsel).into_iter().map(|p| Ask::W(s.weapons[p].idx)).collect(),
            2 => {
                let v = near(self.oview(), self.otop, LIST_VIS, LIST_MARGIN, self.osel);
                if self.otab == 0 { v.into_iter().map(|p| Ask::W(s.weapons[p].idx)).collect() } else { v.into_iter().map(|p| Ask::A(s.pieces[p].idx)).collect() }
            }
            _ => Vec::new(),
        }
    }

    /// 继承页：左边的行；选中的来源的格子（看得见的行 ± 余量，再加选中的那格）；这些格子的放法：这件来源的每条词条，前后几件来源的第一条（按上下键时马上有）
    fn asks_inherit(&self) -> Vec<Ask> {
        let Some(s) = &self.snap else { return Vec::new() };
        let view = self.src_view();
        let mut out: Vec<Ask> = near(view, self.top[0], LIST_VIS, LIST_MARGIN, self.sel[0]).into_iter().map(|p| Ask::A(s.pieces[p].idx)).collect();
        let Some(src) = self.src().filter(|p| p.loaded) else { return out };
        let n = self.cells_len();
        let rows = window(self.top[2] / GRID_COLS, GRID_VIS, n.div_ceil(GRID_COLS), GRID_MARGIN);
        let mut tgts: Vec<u32> = (rows.start * GRID_COLS..(rows.end * GRID_COLS).min(n)).chain([self.sel[2]]).filter_map(|i| self.cell_piece(i)).map(|p| s.pieces[p].idx).collect();
        tgts.sort_unstable();
        tgts.dedup();
        out.extend(tgts.iter().map(|&t| Ask::A(t)));
        for k in 0..self.affs().len() {
            out.extend(tgts.iter().map(|&t| Ask::O(src.idx, k, t)));
        }
        let sel = self.sel[0];
        for i in window(sel, 1, view.len(), SRC_NEAR).filter(|&i| i != sel) {
            let p = &s.pieces[view[i]];
            if p.loaded && s.affs.get(&p.idx).is_some_and(|a| !a.is_empty()) {
                out.extend(tgts.iter().filter(|&&t| t != p.idx).map(|&t| Ask::O(p.idx, 0, t)));
            }
        }
        out
    }

    fn src_view(&self) -> &[usize] {
        &self.v_src
    }

    fn piece_of(&self, pos: usize) -> Option<Piece> {
        self.snap.as_ref()?.pieces.get(pos).cloned()
    }

    fn src(&self) -> Option<Piece> {
        self.piece_of(*self.src_view().get(self.sel[0])?)
    }

    fn affs_of(&self, idx: u32) -> Vec<Aff> {
        self.snap.as_ref().and_then(|s| s.affs.get(&idx).cloned()).unwrap_or_default()
    }

    fn affs(&self) -> Vec<Aff> {
        self.src().map(|p| self.affs_of(p.idx)).unwrap_or_default()
    }

    fn aff(&self) -> Option<Aff> {
        self.affs().get(self.sel[1]).cloned()
    }

    /// 右边的格数：提取成护符（有的话）+ 除了来源自己的全部目标
    fn cells_len(&self) -> usize {
        let (Some(s), Some(src)) = (&self.snap, self.src()) else { return 0 };
        let k = self.sel[1];
        if k >= self.affs().len() {
            return 0;
        }
        s.tal.contains_key(&(src.idx, k)) as usize + self.v_tgt.len() - self.tgt_rank.contains_key(&src.idx) as usize
    }

    /// 第 i 格是哪件目标（pieces 里的位置；提取成护符那格是 None）
    fn cell_piece(&self, i: usize) -> Option<usize> {
        let (s, src) = (self.snap.as_ref()?, self.src()?);
        let mut j = i;
        if s.tal.contains_key(&(src.idx, self.sel[1])) {
            j = j.checked_sub(1)?;
        }
        if self.tgt_rank.get(&src.idx).is_some_and(|&r| j >= r) {
            j += 1;
        }
        let _ = s;
        self.v_tgt.get(j).copied()
    }

    fn cell_at(&self, i: usize) -> Option<Cell> {
        let (s, src) = (self.snap.as_ref()?, self.src()?);
        let k = self.sel[1];
        if k >= self.affs().len() {
            return None;
        }
        if i == 0 {
            if let Some(t) = s.tal.get(&(src.idx, k)) {
                return Some(Cell::Tal { ok: t.ok, icon: t.icon, text: t.text.clone() });
            }
        }
        let t = s.pieces.get(self.cell_piece(i)?)?;
        let key = (src.idx, k, t.idx);
        let (ok, why, known) = if let Some(w) = s.nopt.get(&key) {
            (false, w.clone(), true)
        } else if let Some(v) = s.opts.get(&key) {
            match v.iter().any(|o| o.ok) {
                true => (true, String::new(), true),
                false => (false, v.first().map(|o| o.why.clone()).unwrap_or_default(), true),
            }
        } else {
            (false, String::new(), !s.lazy)
        };
        Some(Cell::Tgt { p: t.clone(), ok, why, known })
    }

    fn cell(&self) -> Option<Cell> {
        self.cell_at(self.sel[2])
    }

    /// 中下的各行（选中的格子）
    fn slots(&self) -> Vec<Slot> {
        let (Some(s), Some(src), Some(cell)) = (&self.snap, self.src(), self.cell()) else { return Vec::new() };
        let k = self.sel[1];
        let l = self.lang();
        match cell {
            Cell::Tal { ok, text, .. } => {
                vec![Slot { label: self.t("护符", "Talisman").into(), aff: None, act: Some((-3, ok, if ok { String::new() } else { text })) }]
            }
            Cell::Tgt { p, .. } => {
                let affs = self.affs_of(p.idx);
                let opts: Vec<Opt> = s.opts.get(&(src.idx, k, p.idx)).cloned().unwrap_or_default();
                let blocked = s.nopt.contains_key(&(src.idx, k, p.idx));
                let mut out: Vec<Slot> = affs
                    .iter()
                    .enumerate()
                    .map(|(i, a)| Slot {
                        label: if a.mech { ["前缀", "Prefix"][l] } else { ["后缀", "Suffix"][l] }.into(),
                        aff: Some(a.clone()),
                        act: opts.iter().find(|o| o.rep == i as i32).map(|o| (o.rep, o.ok, o.why.clone())),
                    })
                    .collect();
                let fill = opts.iter().find(|o| o.rep == -1);
                for j in affs.len()..3 {
                    out.push(Slot {
                        label: ["空位", "Empty"][l].into(),
                        aff: None,
                        act: if j == affs.len() && !blocked { fill.map(|o| (-1, o.ok, o.why.clone())) } else { None },
                    });
                }
                out
            }
        }
    }

    /// 能选的行（有动作的）在 slots 里的序号
    fn slot_choices(&self) -> Vec<usize> {
        self.slots().iter().enumerate().filter(|(_, s)| s.act.is_some()).map(|(i, _)| i).collect()
    }

    fn slot(&self) -> Option<Slot> {
        let ch = self.slot_choices();
        ch.get(self.sel[3]).and_then(|&i| self.slots().get(i).cloned())
    }

    /// 第一个能放的格子（只看加载了的：碰到还没加载的就停）
    fn first_ok_cell(&self) -> usize {
        for i in 0..self.cells_len() {
            match self.cell_at(i) {
                Some(c) if c.ok() => return i,
                Some(Cell::Tgt { known: false, .. }) => break,
                _ => {}
            }
        }
        0
    }

    fn first_ok_slot(&self) -> usize {
        let sl = self.slots();
        self.slot_choices().iter().position(|&i| sl[i].act.as_ref().is_some_and(|a| a.1)).unwrap_or(0)
    }

    /// 每帧：选中的位置收拢到范围里；明细是后来的，还没进的栏里选中的跟着数据走
    pub fn clamp(&mut self) {
        self.refresh_views();
        if self.kf_slot().is_none() {
            self.head = false;
        }
        let n = self.src_view().len();
        self.sel[0] = self.sel[0].min(n.saturating_sub(1));
        if !self.src().is_some_and(|p| p.loaded) {
            if n == 0 {
                self.col = 0;
            }
            return;
        }
        if self.sel[1] >= self.affs().len() {
            self.sel[1] = 0;
        }
        if self.col < 2 || self.sel[2] >= self.cells_len() {
            self.sel[2] = self.first_ok_cell();
        }
        if self.col < 3 || self.sel[3] >= self.slot_choices().len() {
            self.sel[3] = self.first_ok_slot();
        }
        if self.col == 3 && self.slot_choices().is_empty() {
            self.col = 2;
        }
        if self.col >= 1 && self.affs().is_empty() {
            self.col = 0;
        }
    }

    fn select(&mut self, col: usize, i: usize) {
        if self.sel[col] == i {
            return;
        }
        self.sel[col] = i;
        if col == 0 {
            self.sel[1] = 0;
        }
        if col <= 1 {
            self.sel[2] = self.first_ok_cell();
        }
        if col <= 2 {
            self.sel[3] = self.first_ok_slot();
        }
    }

    fn len(&self, col: usize) -> usize {
        match col {
            0 => self.src_view().len(),
            1 => self.affs().len(),
            2 => self.cells_len(),
            _ => self.slot_choices().len(),
        }
    }

    /// 能不能进第 col 栏
    fn can_enter(&self, col: usize) -> bool {
        match col {
            0 => true,
            1 => self.len(0) > 0 && self.len(1) > 0,
            2 => self.len(1) > 0 && self.len(2) > 0,
            _ => matches!(self.cell(), Some(Cell::Tgt { .. })) && self.len(3) > 0,
        }
    }

    fn can_apply(&self) -> bool {
        match self.cell() {
            Some(Cell::Tal { ok, .. }) => ok,
            Some(Cell::Tgt { .. }) => self.slot().and_then(|s| s.act).is_some_and(|a| a.1),
            None => false,
        }
    }

    fn apply(&mut self) {
        self.confirm = false;
        let (Some(src), Some(cell)) = (self.src(), self.cell()) else { return };
        let k = self.sel[1];
        let req = match cell {
            Cell::Tal { ok: true, .. } => Req::Apply { src: src.idx, k, target: None, replace: None },
            Cell::Tgt { p, .. } => match self.slot().and_then(|s| s.act) {
                Some((rep, true, _)) => Req::Apply { src: src.idx, k, target: Some(p.idx), replace: (rep >= 0).then_some(rep as usize) },
                _ => return,
            },
            _ => return,
        };
        self.reqs.push(req);
        self.waiting = true;
        self.msg = Some((true, self.t("处理中……", "Working…").to_string()));
    }

    /// 确认框弹出够久了，可以按「确定」（防止连按两下、按住的键一下子确认掉）
    fn confirm_ready(&self) -> bool {
        self.confirm_t >= CONFIRM_GUARD
    }

    /// 执行完（dark_core 回了结果）：成功就回到左栏
    pub fn done(&mut self, ok: bool, text: String) {
        self.msg = Some((ok, text));
        self.waiting = false;
        if ok && self.page == 0 {
            self.col = 0;
            self.sel[1] = 0;
            self.sel[2] = 0;
            self.sel[3] = 0;
        }
    }

    // ------------------------------------------------------------ 操作
    /// 手柄（按下的那一下 + 连发，lib.rs 算好）、键盘合成这一帧的操作
    /// typing = 正在往输入框里打字（键盘这时不当操作，手柄照常）
    pub fn keys(ui: &Ui, pressed: u16, typing: bool) -> Act {
        // 方向键按住连发（翻列表）；其他键按一下只算一下：按住回车不会一路点到确定（以前 ImGui 默认连发，每 50 毫秒算按一次）
        let k = |key: Key| !typing && ui.is_key_pressed(key);
        let once = |key: Key| !typing && ui.is_key_pressed_no_repeat(key);
        let ctrl = ui.io().key_ctrl;
        Act {
            up: pressed & P_UP != 0 || k(Key::UpArrow),
            down: pressed & P_DOWN != 0 || k(Key::DownArrow),
            left: pressed & P_LEFT != 0 || k(Key::LeftArrow),
            right: pressed & P_RIGHT != 0 || k(Key::RightArrow),
            lb: pressed & P_LB != 0 || once(Key::PageUp),
            rb: pressed & P_RB != 0 || once(Key::PageDown),
            a: pressed & P_A != 0 || once(Key::Enter) || once(Key::KeypadEnter) || once(Key::Space),
            b: pressed & P_B != 0 || once(Key::Escape) || once(Key::Backspace),
            x: pressed & P_X != 0 || once(Key::Tab),
            y: pressed & P_Y != 0 || once(Key::R),
            lt: pressed & P_LT != 0 || once(Key::Q),
            rt: pressed & P_RT != 0 || once(Key::E),
            filter: pressed & P_START != 0 || (once(Key::F) && !ctrl),
            search: once(Key::Slash) || (once(Key::F) && ctrl),
        }
    }

    pub fn handle(&mut self, a: Act) {
        if a != Act::default() {
            self.follow = true;
        }
        if self.waiting || self.snap.is_none() {
            if a.b && !self.waiting {
                self.reqs.push(Req::Close);
            }
            return;
        }
        if self.confirm {
            if (a.a || a.y) && self.confirm_ready() {
                match self.page {
                    1 => self.reforge_go(),
                    2 => self.overlimit_go(),
                    _ => self.apply(),
                }
            } else if a.b {
                self.confirm = false;
            }
            return;
        }
        if a.search {
            self.focus_search = true;
            return;
        }
        if self.picker.is_some() {
            return self.handle_picker(a);
        }
        if self.sort_menu.is_some() {
            return self.handle_sort_menu(a);
        }
        if a.filter {
            return self.open_picker();
        }
        if a.lt || a.rt {
            self.head = false;
            let ps = self.pages();
            let i = ps.iter().position(|&p| p == self.page).unwrap_or(0);
            self.page = ps[if a.rt { (i + 1) % ps.len() } else { (i + ps.len() - 1) % ps.len() }];
            return;
        }
        if self.head {
            return self.handle_head(a);
        }
        if a.up && self.at_list_top() {
            self.head = true;
            self.head_at = 2;
            return;
        }
        if self.page == 1 {
            return self.handle_reforge(a);
        }
        if self.page == 2 {
            return self.handle_overlimit(a);
        }
        if self.page == 3 {
            return self.handle_seal(a);
        }
        if self.page == 4 {
            return self.handle_curse(a);
        }
        if self.page == 5 {
            return self.handle_codex(a);
        }
        let c = self.col;
        let n = self.len(c);
        if c == 2 {
            // 格子：上下一行、左右一格，左边到头回词条栏
            let cols = GRID_COLS;
            let i = self.sel[2];
            if a.up && i >= cols {
                self.select(2, i - cols);
            }
            if a.down && i + cols < n {
                self.select(2, i + cols);
            } else if a.down && i / cols < (n.saturating_sub(1)) / cols {
                self.select(2, n - 1);
            }
            if a.right && i + 1 < n && (i + 1) % cols != 0 {
                self.select(2, i + 1);
            }
            if a.left {
                if i % cols == 0 {
                    self.col = 1;
                } else {
                    self.select(2, i - 1);
                }
            }
        } else {
            if a.up && self.sel[c] > 0 {
                self.select(c, self.sel[c] - 1);
            }
            if a.down && self.sel[c] + 1 < n {
                self.select(c, self.sel[c] + 1);
            }
            if a.left && c > 0 {
                self.col = c - 1;
            }
            if a.right && c < 3 && self.can_enter(c + 1) {
                self.col = c + 1;
            }
        }
        if a.lb && c > 0 {
            self.col = c - 1;
        }
        if a.rb && c < 3 && self.can_enter(c + 1) {
            self.col = c + 1;
        }
        if a.x {
            self.filter = (self.filter + 1) % 5;
            self.sel = [0; 4];
            self.top = [0; 4];
            self.col = 0;
        }
        if a.a {
            match c {
                0 | 1 if self.can_enter(c + 1) => self.col = c + 1,
                2 => match self.cell() {
                    Some(Cell::Tal { ok: true, .. }) => self.confirm = true,
                    Some(Cell::Tgt { .. }) if self.can_enter(3) => self.col = 3,
                    _ => {}
                },
                3 if self.can_apply() => self.confirm = true,
                _ => {}
            }
        }
        if a.y && c >= 2 && self.can_apply() {
            self.confirm = true;
        }
        if a.b {
            if c > 0 {
                self.col = c - 1;
            } else {
                self.reqs.push(Req::Close);
            }
        }
    }

    /// 滚动：（第一行, 一屏几行, 总行数），都按行算（格子一行 GRID_COLS 格）
    fn scroll_info(&self, sc: Scroll) -> (usize, usize, usize) {
        match sc {
            Scroll::Src => (self.top[0], LIST_VIS, self.src_view().len()),
            Scroll::Cell => (self.top[2] / GRID_COLS, GRID_VIS, self.cells_len().div_ceil(GRID_COLS)),
            Scroll::Wpn => (self.rtop, LIST_VIS, self.wview().len()),
            Scroll::OItem => (self.otop, LIST_VIS, self.oview().len()),
            Scroll::KRow => self.codex_scroll(false),
            Scroll::KRel => self.codex_scroll(true),
            Scroll::Pick => (self.picker.as_ref().map_or(0, |p| p.top), filter::PICK_VIS, self.pick_rows().len()),
        }
    }

    /// 鼠标翻列表：选中的行留在原地（翻出屏幕也不动），列表不再跟着它
    fn set_top(&mut self, sc: Scroll, top: usize) {
        self.follow = false;
        match sc {
            Scroll::Src => self.top[0] = top,
            Scroll::Cell => self.top[2] = top * GRID_COLS,
            Scroll::Wpn => self.rtop = top,
            Scroll::OItem => self.otop = top,
            Scroll::KRow => self.codex_set_top(false, top),
            Scroll::KRel => self.codex_set_top(true, top),
            Scroll::Pick => {
                if let Some(p) = &mut self.picker {
                    p.top = top;
                }
            }
        }
    }

    fn wheel(&mut self, sc: Scroll, dir: i32) {
        let (top, vis, n) = self.scroll_info(sc);
        let step = if sc == Scroll::Cell { 1 } else { WHEEL_ROWS as i32 };
        self.set_top(sc, (top as i32 + dir * step).clamp(0, n.saturating_sub(vis) as i32) as usize);
    }

    /// 拖滚动条：滑块中心跟着指针（my 是屏幕坐标）
    fn drag_to(&mut self, sc: Scroll, my: f32) {
        let Some(&(_, y, h, vis, n)) = self.bars.iter().find(|b| b.0 == sc) else { return };
        if n <= vis {
            return;
        }
        let th = (h * vis as f32 / n as f32).max(30.0);
        let t = (((my - self.o[1]) / self.s - y - th / 2.0) / (h - th)).clamp(0.0, 1.0);
        self.set_top(sc, (t * (n - vis) as f32).round() as usize);
    }

    /// 指针停在一行上一小会儿（路过的不算）：只在当前栏里选中，别的栏要点；指针没动过不算（手柄翻页时列表从不动的指针下面滚过）
    fn hover_step(&mut self, hit: Option<Hit>, moved: bool, dt: f32) {
        if hit != self.hover {
            self.hover = hit;
            self.hover_t = 0.0;
            self.hover_moved = false;
        }
        self.hover_moved |= moved;
        self.hover_t += dt;
        if !self.hover_moved || self.hover_t < HOVER_SECS || self.confirm || self.waiting {
            return;
        }
        self.hover_moved = false;
        match hit {
            Some(Hit::Src(i)) if self.col == 0 => self.select(0, i),
            Some(Hit::Aff(i)) if self.col == 1 => self.select(1, i),
            Some(Hit::Cell(i)) if self.col == 2 => self.select(2, i),
            Some(Hit::Slot(i)) if self.col == 3 => self.select(3, i),
            Some(Hit::Wpn(i)) if self.rcol == 0 => self.rsel = i,
            Some(Hit::OItem(i)) if self.ocol == 0 => self.osel = i,
            Some(Hit::OBtn(i)) if self.ocol == 1 => self.obtn = i,
            Some(Hit::NFam(i)) if self.ncol == 0 => self.nsel = i,
            Some(Hit::NSlot(i)) if self.ncol == 1 => self.nslot = i,
            Some(Hit::CRow(i)) => self.csel = i,
            Some(h @ (Hit::KRow(_) | Hit::KRel(_))) => self.hover_codex(h),
            Some(Hit::PRow(i)) => {
                if let Some(p) = &mut self.picker {
                    p.sel = i;
                }
            }
            _ => {}
        }
    }

    /// 鼠标：停留选中、滚轮、拖滚动条、左键（点中的那个选上，点已经选中的 = A）；返回右键（= B），和手柄、键盘的操作合起来
    fn mouse(&mut self, ui: &Ui) -> Act {
        let mut out = Act::default();
        let io = ui.io();
        let p = io.mouse_pos;
        let (confirm, picker, menu) = (self.confirm, self.picker.is_some(), self.sort_menu.is_some());
        let allowed = |h: Hit| {
            if confirm {
                matches!(h, Hit::Ok | Hit::Cancel)
            } else if picker {
                matches!(h, Hit::PRow(_) | Hit::Area(Scroll::Pick) | Hit::Bar(Scroll::Pick) | Hit::Guide(_))
            } else if menu {
                matches!(h, Hit::SortRow(_) | Hit::SortBtn | Hit::Guide(_))
            } else {
                true
            }
        };
        let hit = self.hits.iter().rev().filter(|x| allowed(x.1)).find(|(r, _)| p[0] >= r[0] && p[0] < r[2] && p[1] >= r[1] && p[1] < r[3]).map(|x| x.1);
        if let Some(sc) = self.drag {
            if ui.is_mouse_down(MouseButton::Left) {
                self.drag_to(sc, p[1]);
                return out;
            }
            self.drag = None;
        }
        // hudhook 处理原始输入时把竖向滚轮放在横向上（WM_MOUSEWHEEL 那路没问题），两个都看
        let wheel = if io.mouse_wheel != 0.0 { io.mouse_wheel } else { io.mouse_wheel_h };
        if wheel != 0.0 && !self.confirm {
            if let Some(sc) = hit.and_then(scroll_of) {
                self.wheel(sc, if wheel > 0.0 { -1 } else { 1 });
            }
        }
        self.hover_step(hit, io.mouse_delta != [0.0, 0.0], io.delta_time);
        if self.waiting {
            return out;
        }
        out.b = ui.is_mouse_clicked(MouseButton::Right);
        if !ui.is_mouse_clicked(MouseButton::Left) {
            return out;
        }
        // 排序的下拉菜单开着：点别处只是关掉它
        if self.sort_menu.is_some() && hit.is_none() {
            self.sort_menu = None;
            return out;
        }
        // 点空白处、滚动条不把列表拉回选中的行
        if !matches!(hit, None | Some(Hit::Area(_) | Hit::Bar(_))) {
            self.follow = true;
        }
        if !matches!(hit, None | Some(Hit::Area(_) | Hit::Bar(_) | Hit::Guide(_))) {
            self.head = false;
        }
        let a = Act { a: true, ..Act::default() };
        match hit {
            Some(Hit::Ok) if !self.confirm_ready() => {}
            Some(Hit::Ok) if self.page == 1 => self.reforge_go(),
            Some(Hit::Ok) if self.page == 2 => self.overlimit_go(),
            Some(Hit::Ok) => self.apply(),
            Some(Hit::Cancel) => self.confirm = false,
            _ if self.confirm => {}
            Some(Hit::Area(_)) => {}
            Some(Hit::Bar(sc)) => {
                self.drag = Some(sc);
                self.drag_to(sc, p[1]);
            }
            Some(Hit::Guide(k)) => self.handle(k),
            Some(Hit::PRow(i)) => self.picker_click(i),
            Some(Hit::KfBtn) => self.open_picker(),
            Some(Hit::KfClear) => self.kf_clear(),
            Some(Hit::QClear) => self.query_clear(),
            Some(Hit::SortBtn) => self.toggle_sort_menu(),
            Some(Hit::SortOrd) => self.toggle_order(),
            Some(Hit::SortRow(i)) => self.pick_sort(i),
            Some(Hit::Page(i)) => self.page = i,
            Some(Hit::WTab(i)) => {
                self.rfilter = i;
                self.rsel = 0;
                self.rtop = 0;
                self.rcol = 0;
            }
            Some(Hit::Wpn(i)) => {
                let again = self.rcol == 0 && self.rsel == i;
                self.rcol = 0;
                self.rsel = i;
                if again {
                    self.handle_reforge(a);
                }
            }
            Some(h @ (Hit::OTab(_) | Hit::OItem(_) | Hit::OBtn(_))) => self.click_overlimit(h),
            Some(h @ (Hit::NFam(_) | Hit::NSlot(_))) => self.click_seal(h),
            Some(h @ Hit::CRow(_)) => self.click_curse(h),
            Some(h @ (Hit::KTab(_) | Hit::KRow(_) | Hit::KRel(_))) => self.click_codex(h),
            Some(Hit::Forge) => {
                self.rcol = 1;
                self.handle_reforge(a);
            }
            Some(Hit::QuickForge) => self.quick_reforge(),
            Some(Hit::Tab(i)) => {
                self.filter = i;
                self.sel = [0; 4];
                self.top = [0; 4];
                self.col = 0;
            }
            Some(h) => {
                let (col, i) = match h {
                    Hit::Src(i) => (0, i),
                    Hit::Aff(i) => (1, i),
                    Hit::Cell(i) => (2, i),
                    Hit::Slot(i) => (3, i),
                    _ => return out,
                };
                let again = self.col == col && self.sel[col] == i;
                self.col = col;
                self.select(col, i);
                if again {
                    self.handle(a);
                }
            }
            None => {}
        }
        out
    }

    /// 指针下面那一项亮一下（最后画，盖在内容上）
    fn draw_hover(&mut self, d: &mut Draw) {
        if self.drag.is_some() {
            return;
        }
        let Some(h) = self.hover else { return };
        let Some(&(r, _)) = self.hits.iter().rev().find(|x| x.1 == h) else { return };
        let (x, y, w, hh) = ((r[0] - self.o[0]) / self.s, (r[1] - self.o[1]) / self.s, (r[2] - r[0]) / self.s, (r[3] - r[1]) / self.s);
        match h {
            Hit::Area(_) | Hit::Bar(_) => {}
            Hit::Cell(_) => {
                let k = w * 1.62;
                d.sprite("MENU_SL_Cursor", x + (w - k) / 2.0, y + (hh - k) / 2.0, k, k, 0.45);
            }
            Hit::Tab(_) | Hit::WTab(_) | Hit::OTab(_) | Hit::KTab(_) => d.sprite("MENU_FL_Tab_Cursor", x + w / 2.0 - 42.0, y + hh / 2.0 - 36.0, 84.0, 72.0, 0.45),
            Hit::Page(_) | Hit::Guide(_) | Hit::Ok | Hit::Cancel | Hit::KfBtn | Hit::KfClear | Hit::QClear | Hit::SortBtn | Hit::SortOrd => d.rect(x, y, w, hh, GOLD, 0.16),
            _ => d.sprite("MENU_FL_Cursor_Wide", x - 8.0, y + 2.0, w + 16.0, hh - 4.0, 0.35),
        }
    }

    /// 画滚动条，记下来给鼠标拖
    #[allow(clippy::too_many_arguments)]
    fn bar(&mut self, d: &mut Draw, sc: Scroll, x: f32, y: f32, h: f32, top: usize, vis: usize, n: usize) {
        d.scrollbar(x, y, h, top, vis, n);
        if n > vis {
            self.bars.push((sc, y, h, vis, n));
            self.hit(x - 8.0, y, 30.0, h, Hit::Bar(sc));
        }
    }

    // ------------------------------------------------------------ 画
    fn p(&self, x: f32, y: f32) -> [f32; 2] {
        [self.o[0] + x * self.s, self.o[1] + y * self.s]
    }

    fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, what: Hit) {
        let a = self.p(x, y);
        let b = self.p(x + w, y + h);
        self.hits.push(([a[0], a[1], b[0], b[1]], what));
    }

    /// 一帧：算操作、画界面。screen = 屏幕大小
    pub fn frame(&mut self, ui: &Ui, tex: &mut dyn Tex, act: Act, screen: [f32; 2]) {
        self.s = (screen[0] / 1920.0).min(screen[1] / 1080.0);
        self.o = [(screen[0] - 1920.0 * self.s) / 2.0, (screen[1] - 1080.0 * self.s) / 2.0];
        self.confirm_t = if self.confirm { self.confirm_t + ui.io().delta_time } else { 0.0 };
        self.refresh_views();
        let m = self.mouse(ui);
        self.handle(Act { b: act.b || m.b, ..act });
        self.clamp();
        self.hits.clear();
        self.bars.clear();
        self.typing = false;
        use hudhook::imgui::{Condition, StyleVar, WindowFlags};
        let pad = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        let border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
        let flags = WindowFlags::NO_DECORATION
            | WindowFlags::NO_MOVE
            | WindowFlags::NO_BACKGROUND
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::NO_SCROLL_WITH_MOUSE
            | WindowFlags::NO_FOCUS_ON_APPEARING
            | WindowFlags::NO_NAV;
        ui.window("##dark_ws").position([0.0, 0.0], Condition::Always).size(screen, Condition::Always).flags(flags).build(|| {
            let _font = self.fonts[self.lang()].map(|f| ui.push_font(f.0));
            let mut d = Draw { ui, tex, s: self.s, o: self.o };
            d.rect(-2000.0, -2000.0, 6000.0, 6000.0, [0, 0, 0], 0.72);
            d.sprite_cover("MENU_BaseMain_01", screen, 0.96);
            self.draw_title(&mut d);
            if self.page == 1 {
                self.draw_reforge(&mut d);
                self.draw_guide(&mut d);
                if self.confirm {
                    self.draw_reforge_confirm(&mut d);
                }
            } else if self.page == 5 {
                self.draw_codex(&mut d);
                self.draw_guide(&mut d);
            } else if self.page == 4 {
                self.draw_curses(&mut d);
                self.draw_guide(&mut d);
            } else if self.page == 3 {
                self.draw_seal(&mut d);
                self.draw_guide(&mut d);
            } else if self.page == 2 {
                self.draw_overlimit(&mut d);
                self.draw_guide(&mut d);
                if self.confirm {
                    self.draw_overlimit_confirm(&mut d);
                }
            } else {
                self.draw_left(&mut d);
                self.draw_affixes(&mut d);
                self.draw_grid(&mut d);
                self.draw_slots(&mut d);
                self.draw_guide(&mut d);
                if self.confirm {
                    self.draw_confirm(&mut d);
                }
            }
            if self.picker.is_some() {
                self.draw_picker(&mut d);
            }
            if self.sort_menu.is_some() {
                self.draw_sort_menu(&mut d);
            }
            self.draw_hover(&mut d);
            if !self.note.is_empty() {
                d.text(30.0, 1052.0, 18.0, DIM, 0.8, &self.note);
            }
        });
        border.pop();
        pad.pop();
        self.request_data();
        self.loading = self.loading.saturating_sub(1);
    }

    /// 标题、页签（LT / RT 或 Q / E 换页，鼠标点；魇印、七咒是单独的入口，只有一页、没有页签）、右上角这一页消耗的材料
    fn draw_title(&mut self, d: &mut Draw) {
        d.sprite("MENU_TitleBase", 40.0, 34.0, 824.0, 85.0, 0.9);
        let title = match self.mode {
            1 => self.t("魇印", "Seals"),
            2 => self.t("咒痕", "Cursemarks"),
            _ => self.t("装备工坊", "Gear Workshop"),
        };
        d.text(96.0, 52.0, 40.0, TEXT, 1.0, title);
        let tw = d.width(title, 40.0);
        if self.pages().len() > 1 {
            let mut x = 96.0 + tw + 30.0;
            d.glyph("KG_XboxSeries_L2", x, 60.0, 34.0);
            x += 46.0;
            let names = [self.t("继承", "Inherit"), self.t("重铸", "Reforge"), self.t("超限", "Overlimit"), self.t("魇印", "Seals"), self.t("咒痕", "Cursemarks"), self.t("词条百科", "Codex")];
            for &i in self.pages() {
                let name = names[i];
                let w = d.width(name, 28.0);
                let on = i == self.page;
                d.text_mid(x, 77.0, 28.0, if on { GOLD } else { DIM }, 1.0, name);
                if on {
                    d.sprite("MENU_Line_01", x - 6.0, 100.0, w + 12.0, 3.0, 0.9);
                }
                self.hit(x - 8.0, 52.0, w + 16.0, 52.0, Hit::Page(i));
                x += w + 34.0;
            }
            d.glyph("KG_XboxSeries_R2", x - 10.0, 60.0, 34.0);
        }
        if let Some(s) = self.snap.as_ref().filter(|_| self.page != 5) {
            // 右上角：这一页消耗的材料（继承 融锻之心、重铸 武器重铸石、超限 这一类的超限锻造石；魇印 魇火数、咒痕 反转了几道，0 不标红）
            let (name, n, icon) = if self.page == 1 {
                (self.t("武器重铸石", "Weapon Reforge Stone"), s.rstones, s.rstone_icon)
            } else if self.page == 4 {
                (self.t("已反转", "Reversed"), s.curses.iter().filter(|c| c.rev && c.need > 0).count() as u32, s.shard_icons[1])
            } else if self.page == 3 {
                (self.t("魇火", "Embers"), s.nm_embers.len() as u32, 0)
            } else if self.page == 2 {
                let k = self.otab.min(1);
                let nm = [self.t("超限锻造石（武器）", "Overlimit Stone (Armament)"), self.t("超限锻造石（防具）", "Overlimit Stone (Armor)")][k];
                (nm, s.ostones[k], s.ostone_icon[k])
            } else {
                (self.t("融锻之心", "Fusion Heart"), s.stones, s.stone_icon)
            };
            let line = format!("{name}  {n}");
            let w = d.width(&line, 26.0);
            d.icon(icon, 1840.0 - w - 58.0, 50.0, 50.0, 1.0);
            d.text_mid(1840.0 - w, 75.0, 26.0, if n > 0 || self.page >= 3 { TEXT } else { ERRC }, 1.0, &line);
        }
        self.draw_search(d);
    }

    // 左：部位页签 + 来源列表
    fn draw_left(&mut self, d: &mut Draw) {
        let (x, y, w) = (70.0, 150.0, 600.0);
        let l = self.lang();
        d.sprite("MENU_BaseMain_02", x - 20.0, y - 10.0, w + 60.0, 820.0, 0.55);
        for (i, name) in TABS.iter().enumerate() {
            let tx = x + 14.0 + i as f32 * 70.0;
            if i == self.filter {
                d.sprite("MENU_FL_Tab_Cursor", tx - 12.0, y - 6.0, 84.0, 72.0, 1.0);
            }
            d.sprite_fit(name, tx, y, 60.0, 58.0, if i == self.filter { 1.0 } else { 0.4 });
            self.hit(tx - 6.0, y - 4.0, 70.0, 66.0, Hit::Tab(i));
        }
        d.glyph("KG_XboxSeries_R_L", x + 372.0, y + 12.0, 34.0);
        d.text_mid(x + 412.0, y + 29.0, 24.0, GOLD, 1.0, FILTER[l][self.filter]);
        d.sprite("MENU_Line_01", x, y + 70.0, w, 3.0, 0.8);
        let n = self.src_view().len();
        let (ly, rh, vis) = (y + 82.0, 76.0, LIST_VIS);
        let sel = self.sel[0];
        fit_top(&mut self.top[0], sel, vis, n, self.follow);
        self.hit(x, ly, w, rh * vis as f32, Hit::Area(Scroll::Src));
        if n == 0 {
            let msg = if self.filtering() { self.t("没有符合条件的防具", "No armor matches") } else { self.t("背包里没有能当来源的生成防具（穿着的、锁定的、仓库里的不算）", "No generated armor in your inventory can be a source (worn, locked and stored pieces don't count)") };
            d.wrapped(x + 20.0, ly + 20.0, w - 40.0, 22.0, DIM, 1.0, msg);
        }
        let top = self.top[0];
        let at = |ws: &Self, i: usize| ws.src_view().get(i).copied().and_then(|p| ws.piece_of(p));
        for i in window(top, vis, n, 0) {
            let Some(p) = at(self, i) else { continue };
            let ry = ly + (i - top) as f32 * rh;
            if i == sel {
                d.sprite("MENU_FL_Cursor_Wide", x - 8.0, ry + 2.0, w + 16.0, rh - 4.0, if self.col == 0 && !self.head { 1.0 } else { 0.45 });
            }
            if p.loaded {
                d.icon(p.icon, x + 12.0, ry + 6.0, 64.0, 1.0);
                d.text_clip(x + 92.0, ry + 10.0, w - 100.0, 24.0, RAR[p.rar.min(3)], 1.0, &p.name);
                let c = self.affs_of(p.idx).len();
                let mut sub = if l == 0 { format!("{} · {} 个词条", PART[0].get(p.part).unwrap_or(&"?"), c) } else { format!("{} · {} affix(es)", PART[1].get(p.part).unwrap_or(&"?"), c) };
                // 按减伤率 / 重量排的时候，把这个数也写出来（filter.rs 的排序；稀有度看名字的颜色就知道）
                match self.sort {
                    2 => sub += &format!(" · {}{:.1}%", self.t("减伤率 ", "Negation "), p.cut),
                    3 => sub += &format!(" · {}{:.1}", self.t("重量 ", "Weight "), p.weight),
                    _ => {}
                }
                d.text(x + 92.0, ry + 44.0, 19.0, DIM, 1.0, &sub);
            }
            self.hit(x, ry, w, rh, Hit::Src(i));
        }
        for i in window(top, vis, n, LIST_MARGIN).filter(|&i| i < top || i >= top + vis) {
            if let Some(p) = at(self, i) {
                d.preload(p.icon);
            }
        }
        self.bar(d, Scroll::Src, x + w + 8.0, ly, rh * vis as f32, top, vis, n);
        self.draw_kf_strip(d, x, ly + rh * vis as f32 + 6.0, w);
    }

    // 中上：来源的名字 + 词条
    fn draw_affixes(&mut self, d: &mut Draw) {
        let (x, y, w) = (720.0, 150.0, 560.0);
        d.sprite("MENU_FL_MenuBase", x - 16.0, y - 8.0, w + 32.0, 430.0, 0.9);
        let Some(src) = self.src() else { return };
        let banner = ["MENU_FL_Rar_d0", "MENU_FL_Rar_d1", "MENU_FL_Rar_d1", "MENU_FL_Rar_d2"][src.rar.min(3)];
        d.sprite(banner, x - 10.0, y, w + 20.0, 76.0, 0.9);
        d.text_clip(x + 20.0, y + 10.0, w - 40.0, 26.0, RAR[src.rar.min(3)], 1.0, &src.name);
        d.text(x + 20.0, y + 44.0, 19.0, DIM, 1.0, &format!("{}  ·  {}", self.t("来源（会销毁）", "Source (destroyed)"), RAR_NAME[self.lang()][src.rar.min(3)]));
        let mut ry = y + 92.0;
        let l = self.lang();
        for (i, a) in self.affs().iter().enumerate() {
            let lines = d.wrap(&a.text, w - 60.0, 20.0);
            let h = 40.0 + lines.len() as f32 * 26.0 + 10.0;
            if i == self.sel[1] {
                d.sprite("MENU_FL_Cursor_Wide", x - 8.0, ry, w + 16.0, h, if self.col == 1 { 1.0 } else { 0.45 });
            }
            let lab = if a.mech { ["前缀", "Prefix"][l] } else { ["后缀", "Suffix"][l] };
            d.text(x + 20.0, ry + 10.0, 19.0, GOLD, 1.0, lab);
            let lw = d.width(lab, 19.0);
            d.text(x + 32.0 + lw, ry + 7.0, 24.0, RAR[a.rar.min(3)], 1.0, &a.name);
            let nw = d.width(&a.name, 24.0);
            d.text(x + 44.0 + lw + nw, ry + 11.0, 18.0, DIM, 1.0, RAR_NAME[l][a.rar.min(3)]);
            for (j, t) in lines.iter().enumerate() {
                d.text(x + 32.0, ry + 40.0 + j as f32 * 26.0, 20.0, TEXT, 0.92, t);
            }
            self.hit(x, ry, w, h, Hit::Aff(i));
            ry += h + 4.0;
        }
    }

    // 右：目标格子 + 选中那格的说明
    fn draw_grid(&mut self, d: &mut Draw) {
        let (x, y) = (1320.0, 150.0);
        let total = self.cells_len();
        d.text(x, y + 6.0, 26.0, GOLD, 1.0, self.t("放到哪件上", "Destination"));
        let n = total - matches!(self.cell_at(0), Some(Cell::Tal { .. })) as usize;
        let cnt = if self.lang() == 0 { format!("{n} 件") } else { format!("{n} pieces") };
        d.text(x + d.width(self.t("放到哪件上", "Destination"), 26.0) + 16.0, y + 12.0, 19.0, DIM, 1.0, &cnt);
        d.sprite("MENU_Line_01", x, y + 46.0, 520.0, 3.0, 0.8);
        if total == 0 {
            d.wrapped(x + 10.0, y + 70.0, 500.0, 22.0, DIM, 1.0, self.t("（先在左边选来源和词条）", "(pick a source and an affix first)"));
            return;
        }
        let (gy, cs, gap, vis_rows) = (y + 62.0, 96.0, 8.0, GRID_VIS);
        let rows = total.div_ceil(GRID_COLS);
        let sel_row = self.sel[2] / GRID_COLS;
        let mut top_row = self.top[2] / GRID_COLS;
        fit_top(&mut top_row, sel_row, vis_rows, rows, self.follow);
        self.top[2] = top_row * GRID_COLS;
        self.hit(x, gy, GRID_COLS as f32 * (cs + gap), vis_rows as f32 * (cs + gap), Hit::Area(Scroll::Cell));
        for i in top_row * GRID_COLS..((top_row + vis_rows) * GRID_COLS).min(total) {
            let Some(cell) = self.cell_at(i) else { continue };
            let cx = x + (i % GRID_COLS) as f32 * (cs + gap);
            let cy = gy + (i / GRID_COLS - top_row) as f32 * (cs + gap);
            d.sprite("MENU_SlotBase", cx - 4.0, cy - 4.0, cs + 8.0, cs + 8.0, 0.85);
            // 放法还没加载的不当成放不了
            let a = if cell.ok() { 1.0 } else if matches!(cell, Cell::Tgt { known: false, .. }) { 0.65 } else { 0.3 };
            match &cell {
                Cell::Tal { icon, .. } => {
                    d.icon(*icon, cx + 4.0, cy + 4.0, cs - 8.0, a);
                    d.rect(cx + 4.0, cy + cs - 30.0, cs - 8.0, 26.0, [12, 10, 6], 0.75);
                    let lab = self.t("提取护符", "Talisman");
                    let lw = d.width(lab, 17.0);
                    d.text(cx + (cs - lw) / 2.0, cy + cs - 28.0, 17.0, GOLD, a.max(0.6), lab);
                }
                Cell::Tgt { p, .. } => {
                    d.icon(p.icon, cx + 4.0, cy + 4.0, cs - 8.0, a);
                    d.badges(cx + 6.0, cy + 6.0, 26.0, cs - 8.0, p.worn, p.locked, false);
                }
            }
            if i == self.sel[2] {
                // 原版格子光标：亮框只占贴图中间约 2/3，放大到框正好套住格子
                let k = cs * 1.62;
                d.sprite("MENU_SL_Cursor", cx + (cs - k) / 2.0, cy + (cs - k) / 2.0, k, k, if self.col == 2 { 1.0 } else { 0.55 });
            }
            self.hit(cx, cy, cs, cs, Hit::Cell(i));
        }
        let edge = window(top_row, vis_rows, rows, GRID_MARGIN);
        for i in (edge.start * GRID_COLS..(edge.end * GRID_COLS).min(total)).filter(|&i| i / GRID_COLS < top_row || i / GRID_COLS >= top_row + vis_rows) {
            if let Some(p) = self.cell_piece(i).and_then(|p| self.piece_of(p)) {
                d.preload(p.icon);
            }
        }
        self.bar(d, Scroll::Cell, x + GRID_COLS as f32 * (cs + gap) + 2.0, gy, vis_rows as f32 * (cs + gap), top_row, vis_rows, rows);
        // 说明
        let iy = gy + vis_rows as f32 * (cs + gap) + 14.0;
        d.sprite("MENU_Line_01", x, iy, 520.0, 3.0, 0.6);
        match self.cell() {
            Some(Cell::Tal { ok, text, .. }) => {
                d.text(x, iy + 14.0, 24.0, GOLD, 1.0, self.t("提取成护符", "Extract as talisman"));
                d.wrapped(x, iy + 50.0, 520.0, 22.0, if ok { TEXT } else { ERRC }, 1.0, &text);
            }
            Some(Cell::Tgt { p, ok, why, .. }) => {
                d.text_clip(x, iy + 14.0, 520.0, 24.0, RAR[p.rar.min(3)], 1.0, &p.name);
                let part = PART[self.lang()].get(p.part).copied().unwrap_or("?");
                d.text(x, iy + 46.0, 19.0, DIM, 1.0, part);
                d.badges_row(x + d.width(part, 19.0) + 10.0, iy + 42.0, 26.0, p.worn, p.locked);
                let mut ly = iy + 76.0;
                for a in self.affs_of(p.idx) {
                    d.text_clip(x + 8.0, ly, 512.0, 20.0, RAR[a.rar.min(3)], 1.0, &format!("{} {}", if a.mech { "◆" } else { "◇" }, a.name));
                    ly += 28.0;
                }
                if !ok && !why.is_empty() {
                    d.wrapped(x + 8.0, ly + 4.0, 512.0, 20.0, ERRC, 1.0, &why);
                }
            }
            None => {}
        }
    }

    // 中下：位置 + 预览 + 消耗
    fn draw_slots(&mut self, d: &mut Draw) {
        let (x, y, w) = (720.0, 600.0, 560.0);
        d.sprite("MENU_FL_MenuBase", x - 16.0, y - 8.0, w + 32.0, 370.0, 0.9);
        let Some(cell) = self.cell() else { return };
        let l = self.lang();
        let head = match &cell {
            Cell::Tal { .. } => self.t("提取成护符", "Extract as talisman").to_string(),
            Cell::Tgt { p, .. } => format!("{} {}", self.t("放到", "Onto"), p.name),
        };
        d.text_clip(x + 4.0, y + 6.0, w - 8.0, 22.0, GOLD, 1.0, &head);
        d.sprite("MENU_Line_01", x, y + 38.0, w, 3.0, 0.7);
        let slots = self.slots();
        let choices = self.slot_choices();
        let chosen = choices.get(self.sel[3]).copied();
        let mut ry = y + 48.0;
        for (i, s) in slots.iter().enumerate() {
            let h = 40.0;
            if Some(i) == chosen {
                d.sprite("MENU_FL_Cursor_Wide", x - 8.0, ry, w + 16.0, h, if self.col == 3 { 1.0 } else { 0.45 });
            }
            let dim = if s.act.is_some() { 1.0 } else { 0.45 };
            d.text(x + 14.0, ry + 9.0, 19.0, GOLD, dim, &s.label);
            match &s.aff {
                Some(a) => d.text_clip(x + 80.0, ry + 7.0, 280.0, 22.0, RAR[a.rar.min(3)], dim, &a.name),
                None if matches!(cell, Cell::Tal { .. }) => {
                    if let Some(a) = self.aff() {
                        d.text_clip(x + 80.0, ry + 7.0, 280.0, 22.0, RAR[a.rar.min(3)], 1.0, &a.name)
                    }
                }
                None => d.text(x + 80.0, ry + 8.0, 20.0, DIM, dim, self.t("（空）", "(empty)")),
            }
            let (txt, col) = match &s.act {
                Some((-3, true, _)) => (self.t("提取", "Extract").to_string(), OKC),
                Some((-1, true, _)) => (self.t("补上", "Fill").to_string(), OKC),
                Some((_, true, _)) => (self.t("替换", "Replace").to_string(), OKC),
                Some((_, false, why)) => (why.clone(), ERRC),
                None => (String::new(), DIM),
            };
            if !txt.is_empty() {
                let tw = d.width(&txt, 19.0).min(190.0);
                d.text_clip(x + w - 12.0 - tw, ry + 10.0, 190.0, 19.0, col, 1.0, &txt);
            }
            if s.act.is_some() {
                self.hit(x, ry, w, h, Hit::Slot(choices.iter().position(|&c| c == i).unwrap_or(0)));
            }
            ry += h + 2.0;
        }
        if let (Cell::Tgt { why, .. }, true) = (&cell, choices.is_empty()) {
            d.wrapped(x + 14.0, ry + 4.0, w - 28.0, 20.0, ERRC, 1.0, if why.is_empty() { self.t("这件放不了", "Can't go on this piece") } else { why });
            ry += 30.0;
        }
        // 之后的样子
        ry += 8.0;
        if let (Cell::Tal { ok: true, icon, text }, Some(a)) = (&cell, self.aff()) {
            d.text(x + 14.0, ry, 19.0, DIM, 1.0, self.t("得到", "You get"));
            d.icon(*icon, x + 76.0, ry - 6.0, 72.0, 1.0);
            d.text_clip(x + 160.0, ry, w - 170.0, 22.0, RAR[a.rar.min(3)], 1.0, text);
            d.wrapped(x + 160.0, ry + 34.0, w - 170.0, 19.0, TEXT, 0.9, &a.text);
        }
        if let (Cell::Tgt { p, .. }, Some(s), Some(src_aff)) = (&cell, self.slot(), self.aff()) {
            if let Some((rep, true, _)) = s.act {
                d.text(x + 14.0, ry, 19.0, DIM, 1.0, self.t("之后", "After"));
                let mut after: Vec<(bool, Aff)> = self.affs_of(p.idx).into_iter().map(|a| (false, a)).collect();
                if rep >= 0 && (rep as usize) < after.len() {
                    after[rep as usize] = (true, src_aff.clone());
                } else {
                    after.push((true, src_aff.clone()));
                }
                after.sort_by_key(|a| !a.1.mech);
                let mut ay = ry;
                for (new, a) in after {
                    let mark = if a.mech { "◆" } else { "◇" };
                    d.text_clip(x + 80.0, ay, 380.0, 20.0, RAR[a.rar.min(3)], 1.0, &format!("{mark} {}", a.name));
                    if new {
                        d.text(x + 470.0, ay + 1.0, 18.0, OKC, 1.0, self.t("新", "new"));
                    }
                    ay += 26.0;
                }
                if p.worn {
                    d.text(x + 14.0, ay + 2.0, 18.0, DIM, 1.0, self.t("穿着的：换下再穿上后生效", "Worn: re-equip for the change to apply"));
                }
            }
        }
        // 消耗
        if let (Some(src), Some(s)) = (self.src(), &self.snap) {
            let cy = y + 322.0;
            d.sprite("MENU_Line_01", x, cy - 8.0, w, 3.0, 0.5);
            d.icon(s.stone_icon, x + 8.0, cy - 2.0, 36.0, 1.0);
            let line = if l == 0 {
                format!("消耗 融锻之心 ×1（现有 {}）＋ 来源「{}」", s.stones, src.name)
            } else {
                format!("Uses 1 Fusion Heart (have {}) + the source \"{}\"", s.stones, src.name)
            };
            d.text_clip_mid(x + 50.0, cy + 16.0, w - 56.0, 19.0, if s.stones > 0 { TEXT } else { ERRC }, 1.0, &line);
        }
    }

    fn draw_guide(&mut self, d: &mut Draw) {
        let mut items: Vec<(&str, String)> = Vec::new();
        let col = match self.page {
            1 => self.rcol,
            2 => self.ocol,
            3 => self.ncol,
            4 => 0,
            5 => self.kcol,
            _ => self.col,
        };
        let a = match (self.page, col) {
            (5, 0) if !self.codex_has_related() => "",
            (5, 0) => self.t("相关词条", "Related"),
            (5, _) => self.t("查看", "View"),
            (1..=4, 0) => self.t("选择", "Select"),
            (3, _) => self.t("刻上", "Engrave"),
            (1, _) => self.t("重铸", "Reforge"),
            (2, _) if self.oaff_sel().is_some() => self.t("升级", "Upgrade"),
            (2, _) => self.t("超限", "Overlimit"),
            (_, 0 | 1) => self.t("选择", "Select"),
            (_, 2) if matches!(self.cell(), Some(Cell::Tal { .. })) => self.t("提取", "Extract"),
            (_, 2) => self.t("选位置", "Choose slot"),
            _ => self.t("确定", "Confirm"),
        };
        if !a.is_empty() {
            items.push(("KG_XboxSeries_R_D", a.to_string()));
        }
        items.push(("KG_XboxSeries_R_R", if col == 0 && !self.codex_can_back() { self.t("关闭", "Close") } else { self.t("返回", "Back") }.to_string()));
        if self.page == 0 {
            items.push(("KG_XboxSeries_L1", String::new()));
            items.push(("KG_XboxSeries_R1", self.t("换栏", "Column").to_string()));
        }
        if self.page == 1 && self.can_quick_reforge() {
            items.push(("KG_XboxSeries_R_U", self.t("快速重铸", "Quick reforge").to_string()));
        }
        let x_label = match self.page {
            1 => self.t("类别", "Category"),
            2 => self.t("武器 / 防具", "Armament / Armor"),
            3 => self.t("取下", "Remove"),
            4 => "",
            5 => self.t("类别", "Category"),
            _ => self.t("部位", "Slot filter"),
        };
        if !x_label.is_empty() {
            items.push(("KG_XboxSeries_R_L", x_label.to_string()));
        }
        if self.pages().len() > 1 {
            items.push(("KG_XboxSeries_L2", String::new()));
            items.push(("KG_XboxSeries_R2", self.t("换页", "Page").to_string()));
        }
        if self.kf_slot().is_some() {
            items.push(("KG_XboxSeries_Start", self.t("词条筛选", "Affix filter").to_string()));
        }
        if self.picker.is_some() {
            items = vec![
                ("KG_XboxSeries_R_D", self.t("选择", "Select").to_string()),
                ("KG_XboxSeries_R_L", self.t("全部清除", "Clear all").to_string()),
                ("KG_XboxSeries_R_R", self.t("关闭", "Close").to_string()),
            ];
        }
        if self.sort_menu.is_some() {
            items = vec![("KG_XboxSeries_R_D", self.t("选择", "Select").to_string()), ("KG_XboxSeries_R_R", self.t("关闭", "Close").to_string())];
        } else if self.head && self.picker.is_none() && self.page == 0 && self.head_at < 2 {
            let what = if self.head_at == 0 { self.t("选择排序", "Sort by") } else { self.t("升序 / 降序", "Ascending / descending") };
            items = vec![("KG_XboxSeries_R_D", what.to_string())];
            if (self.sort, self.asc) != (0, false) {
                items.push(("KG_XboxSeries_R_L", self.t("恢复默认", "Reset").to_string()));
            }
            items.push(("KG_XboxSeries_R_R", self.t("返回", "Back").to_string()));
        } else if self.head && self.picker.is_none() {
            items = vec![("KG_XboxSeries_R_D", self.t("词条筛选", "Affix filter").to_string())];
            if self.kf_slot().is_some_and(|(k, _)| !self.kf[k].is_empty()) {
                items.push(("KG_XboxSeries_R_L", self.t("清除", "Clear").to_string()));
            }
            items.push(("KG_XboxSeries_R_R", self.t("返回", "Back").to_string()));
        }
        let mut x = 1840.0;
        for (g, label) in items.iter().rev() {
            let tw = if label.is_empty() { 0.0 } else { d.width(label, 22.0) + 22.0 };
            x -= tw;
            if !label.is_empty() {
                d.text_mid(x, 1018.0, 22.0, TEXT, 1.0, label);
            }
            x -= 40.0;
            d.glyph(g, x, 1000.0, 36.0);
            self.hit(x - 2.0, 996.0, 40.0 + tw, 44.0, Hit::Guide(glyph_act(g)));
            x -= 4.0;
        }
        if let Some((ok, m)) = &self.msg {
            d.text_clip_mid(80.0, 1018.0, x - 120.0, 22.0, if *ok { OKC } else { ERRC }, 1.0, m);
        }
    }

    fn draw_confirm(&mut self, d: &mut Draw) {
        let (Some(src), Some(a), Some(cell)) = (self.src(), self.aff(), self.cell()) else { return };
        d.rect(-2000.0, -2000.0, 6000.0, 6000.0, [0, 0, 0], 0.5);
        // 原版对话框那种横条：上下渐隐的深色带 + 上下两条金色光线
        d.band(-100.0, 400.0, 2120.0, 280.0, 0.92);
        d.sprite("MENU_FL_Dialog", -40.0, 410.0, 2000.0, 260.0, 0.8);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 418.0, 1600.0, 22.0, 0.9);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 640.0, 1600.0, 22.0, 0.9);
        let what = match &cell {
            Cell::Tal { text, .. } => format!("{}「{}」", self.t("提取成护符", "extract it as "), text),
            Cell::Tgt { p, .. } => {
                let how = match self.slot().and_then(|s| s.act) {
                    Some((-1, ..)) => self.t("补上", "add it to").to_string(),
                    Some((i, ..)) => {
                        let old = self.affs_of(p.idx).get(i as usize).map(|x| x.name.clone()).unwrap_or_default();
                        if self.lang() == 0 { format!("替换「{old}」") } else { format!("replace \"{old}\" on") }
                    }
                    None => String::new(),
                };
                if self.lang() == 0 { format!("放到「{}」上，{how}", p.name) } else { format!("{how} \"{}\"", p.name) }
            }
        };
        let l1 = if self.lang() == 0 { format!("把「{}」从「{}」{what}", a.name, src.name) } else { format!("Take \"{}\" from \"{}\" and {what}", a.name, src.name) };
        let l2 = self.t("来源会销毁，消耗 1 个融锻之心。", "The source is destroyed and 1 Fusion Heart is used.");
        for (i, t) in [l1.as_str(), l2].iter().enumerate() {
            let tw = d.width(t, 26.0).min(1700.0);
            d.text_clip(960.0 - tw / 2.0, 470.0 + i as f32 * 46.0, 1700.0, 26.0, if i == 0 { TEXT } else { GOLD }, 1.0, t);
        }
        let ok = self.t("确定", "Confirm");
        let no = self.t("取消", "Cancel");
        let (wo, wn) = (d.width(ok, 26.0), d.width(no, 26.0));
        let total = 44.0 + wo + 60.0 + 44.0 + wn;
        let mut bx = 960.0 - total / 2.0;
        d.glyph("KG_XboxSeries_R_D", bx, 575.0, 38.0);
        d.text_mid(bx + 44.0, 594.0, 26.0, if self.confirm_ready() { TEXT } else { DIM }, 1.0, ok);
        self.hit(bx, 570.0, 44.0 + wo, 48.0, Hit::Ok);
        bx += 44.0 + wo + 60.0;
        d.glyph("KG_XboxSeries_R_R", bx, 575.0, 38.0);
        d.text_mid(bx + 44.0, 594.0, 26.0, TEXT, 1.0, no);
        self.hit(bx, 570.0, 44.0 + wn, 48.0, Hit::Cancel);
    }
}

/// 格子每行几个
const GRID_COLS: usize = 5;

/// 左边的列表一屏几行、格子一屏几行
const LIST_VIS: usize = 9;
const GRID_VIS: usize = 4;
/// 明细和图标多预备几行（前后各）：列表、格子；继承页前后几件来源的放法也先要好
const LIST_MARGIN: usize = 6;
const GRID_MARGIN: usize = 1;
const SRC_NEAR: usize = 2;
/// 滚轮一格翻几行；指针停多久算选中（路过的不算）
const WHEEL_ROWS: usize = 3;
const HOVER_SECS: f32 = 0.1;
/// 确认框弹出后这么久才收「确定」
const CONFIRM_GUARD: f32 = 0.3;
/// 工坊用到的键盘键：开窗口时还按着就等全松开才收键盘（lib.rs）
pub const KB_KEYS: [Key; 17] = [
    Key::Escape,
    Key::Backspace,
    Key::Enter,
    Key::KeypadEnter,
    Key::Space,
    Key::Tab,
    Key::UpArrow,
    Key::DownArrow,
    Key::LeftArrow,
    Key::RightArrow,
    Key::PageUp,
    Key::PageDown,
    Key::Q,
    Key::E,
    Key::F,
    Key::R,
    Key::Slash,
];

/// 第一行 top、一屏 vis 行、总共 n 行，前后各留 m 行：这段的行号
fn window(top: usize, vis: usize, n: usize, m: usize) -> Range<usize> {
    top.saturating_sub(m).min(n)..(top + vis + m).min(n)
}

/// 列表（pieces / weapons 的位置）看得见的 ± 余量，再加上选中的那行
fn near(view: &[usize], top: usize, vis: usize, m: usize, sel: usize) -> Vec<usize> {
    let mut v = view[window(top, vis, view.len(), m)].to_vec();
    if let Some(&p) = view.get(sel) {
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// 这一项属于哪个能滚的列表
fn scroll_of(h: Hit) -> Option<Scroll> {
    Some(match h {
        Hit::Area(s) | Hit::Bar(s) => s,
        Hit::Src(_) => Scroll::Src,
        Hit::Cell(_) => Scroll::Cell,
        Hit::Wpn(_) => Scroll::Wpn,
        Hit::OItem(_) => Scroll::OItem,
        Hit::KRow(_) => Scroll::KRow,
        Hit::KRel(_) => Scroll::KRel,
        Hit::PRow(_) => Scroll::Pick,
        _ => return None,
    })
}

/// 底部按键提示的图标 → 点了相当于按哪个键
fn glyph_act(g: &str) -> Act {
    let mut a = Act::default();
    match g {
        "KG_XboxSeries_R_D" => a.a = true,
        "KG_XboxSeries_R_R" => a.b = true,
        "KG_XboxSeries_R_L" => a.x = true,
        "KG_XboxSeries_R_U" => a.y = true,
        "KG_XboxSeries_L1" => a.lb = true,
        "KG_XboxSeries_R1" => a.rb = true,
        "KG_XboxSeries_L2" => a.lt = true,
        "KG_XboxSeries_R2" => a.rt = true,
        "KG_XboxSeries_Start" => a.filter = true,
        _ => {}
    }
    a
}

/// 列表第一行：follow = 让选中的那行在可见范围里（手柄 / 键盘）；鼠标翻的时候只收拢到范围里，选中的行可以翻出去
fn fit_top(top: &mut usize, sel: usize, vis: usize, n: usize, follow: bool) {
    if follow {
        if sel < *top {
            *top = sel;
        }
        if sel >= *top + vis {
            *top = sel + 1 - vis;
        }
    }
    *top = (*top).min(n.saturating_sub(vis));
}

/// 画图、写字（设计坐标）
struct Draw<'a> {
    ui: &'a Ui,
    tex: &'a mut dyn Tex,
    s: f32,
    o: [f32; 2],
}

impl Draw<'_> {
    fn p(&self, x: f32, y: f32) -> [f32; 2] {
        [self.o[0] + x * self.s, self.o[1] + y * self.s]
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3], a: f32) {
        let (p0, p1) = (self.p(x, y), self.p(x + w, y + h));
        self.ui.get_window_draw_list().add_rect(p0, p1, c(rgb, a)).filled(true).build();
    }

    /// 上下渐隐的黑色横条（中间最深 a）
    fn band(&mut self, x: f32, y: f32, w: f32, h: f32, a: f32) {
        let (t, m, b) = (self.p(x, y), self.p(x + w, y + h / 2.0), self.p(x + w, y + h));
        let (clear, dark) = (c([0, 0, 0], 0.0), c([0, 0, 0], a));
        let dl = self.ui.get_window_draw_list();
        dl.add_rect_filled_multicolor(t, m, clear, clear, dark, dark);
        dl.add_rect_filled_multicolor([t[0], m[1]], b, dark, dark, clear, clear);
    }

    fn image(&mut self, img: Img, x: f32, y: f32, w: f32, h: f32, a: f32) {
        let (p0, p1) = (self.p(x, y), self.p(x + w, y + h));
        self.ui.get_window_draw_list().add_image(img.id, p0, p1).col(c([255, 255, 255], a)).build();
    }

    fn sprite(&mut self, name: &str, x: f32, y: f32, w: f32, h: f32, a: f32) {
        if let Some(img) = self.tex.sprite(name) {
            self.image(img, x, y, w, h, a);
        }
    }

    /// 等比放进框里、居中
    fn sprite_fit(&mut self, name: &str, x: f32, y: f32, w: f32, h: f32, a: f32) {
        if let Some(img) = self.tex.sprite(name) {
            let k = (w / img.w).min(h / img.h);
            let (iw, ih) = (img.w * k, img.h * k);
            self.image(img, x + (w - iw) / 2.0, y + (h - ih) / 2.0, iw, ih, a);
        }
    }

    /// 铺满整个屏幕（等比、超出的裁掉）
    fn sprite_cover(&mut self, name: &str, screen: [f32; 2], a: f32) {
        if let Some(img) = self.tex.sprite(name) {
            let k = (screen[0] / img.w).max(screen[1] / img.h);
            let (w, h) = (img.w * k, img.h * k);
            let p0 = [(screen[0] - w) / 2.0, (screen[1] - h) / 2.0];
            self.ui.get_window_draw_list().add_image(img.id, p0, [p0[0] + w, p0[1] + h]).col(c([255, 255, 255], a)).build();
        }
    }

    fn glyph(&mut self, name: &str, x: f32, y: f32, size: f32) {
        self.sprite(name, x, y, size, size, 1.0);
    }

    /// 物品图标上的小标记：「装备中」（左上，原版 MENU_EquMark）、锁定（左上，装备中时下移一格；dark_lock 背包里那个锁）、
    /// 弃置的红叉（右下角，用户 2026-11-26）；h = 标记高度，icon = 物品图标的边长（红叉要贴到图标右下角，位置跟原版「属性不足」区分开）
    fn badges(&mut self, x: f32, y: f32, h: f32, icon: f32, worn: bool, locked: bool, discard: bool) {
        let w = h * 40.0 / 42.0;
        if worn {
            self.sprite("MENU_EquMark", x, y, w, h, 1.0);
        }
        if locked {
            self.sprite("DR_Lock", x, if worn { y + h } else { y }, w, h, 1.0);
        }
        if discard {
            self.sprite("DR_Discard", x + icon - w - 1.0, y + icon - h - 1.0, w, h, 1.0);
        }
    }

    /// 同上，横着排（说明那一行用）
    fn badges_row(&mut self, x: f32, y: f32, h: f32, worn: bool, locked: bool) {
        let w = h * 40.0 / 42.0;
        if worn {
            self.sprite("MENU_EquMark", x, y, w, h, 1.0);
        }
        if locked {
            self.sprite("DR_Lock", x + if worn { w + 4.0 } else { 0.0 }, y, w, h, 1.0);
        }
    }

    fn icon(&mut self, id: u16, x: f32, y: f32, size: f32, a: f32) {
        if id == 0 {
            return;
        }
        if let Some(img) = self.tex.icon(id) {
            self.image(img, x, y, size, size, a);
        }
    }

    fn scale(&self, px: f32) {
        self.ui.set_window_font_scale(px * self.s / FONT_PX);
    }

    /// 文字宽度（设计坐标）
    fn width(&self, t: &str, px: f32) -> f32 {
        self.scale(px);
        self.ui.calc_text_size(t)[0] / self.s
    }

    fn text(&mut self, x: f32, y: f32, px: f32, rgb: [u8; 3], a: f32, t: &str) {
        self.scale(px);
        let p = self.p(x, y);
        self.ui.get_window_draw_list().add_text(p, c(rgb, a), t);
    }

    /// 超过 w 的截断加省略号
    fn text_clip(&mut self, x: f32, y: f32, w: f32, px: f32, rgb: [u8; 3], a: f32, t: &str) {
        if self.width(t, px) <= w {
            return self.text(x, y, px, rgb, a, t);
        }
        let mut s: String = t.to_string();
        while !s.is_empty() && self.width(&format!("{s}…"), px) > w {
            s.pop();
        }
        self.text(x, y, px, rgb, a, &format!("{s}…"));
    }

    /// 文字的中线（字的墨迹上下居中）放在 cy：和按键图标并排的文字用它，cy = 图标的 y + 大小 / 2
    fn text_mid(&mut self, x: f32, cy: f32, px: f32, rgb: [u8; 3], a: f32, t: &str) {
        self.text(x, cy - TEXT_MID * px, px, rgb, a, t);
    }

    fn text_clip_mid(&mut self, x: f32, cy: f32, w: f32, px: f32, rgb: [u8; 3], a: f32, t: &str) {
        self.text_clip(x, cy - TEXT_MID * px, w, px, rgb, a, t);
    }

    /// 按宽度折行（中文每个字都能断，英文在空格断）
    fn wrap(&self, t: &str, w: f32, px: f32) -> Vec<String> {
        let mut out = Vec::new();
        for para in t.split('\n') {
            let mut line = String::new();
            let mut word = String::new();
            let flush = |line: &mut String, word: &mut String, out: &mut Vec<String>| {
                let cand = format!("{line}{word}");
                if self.width(cand.trim_end(), px) > w && !line.is_empty() {
                    out.push(line.trim_end().to_string());
                    *line = word.trim_start().to_string();
                } else {
                    *line = cand;
                }
                word.clear();
            };
            for ch in para.chars() {
                word.push(ch);
                if ch == ' ' || (ch as u32) >= 0x2E80 {
                    flush(&mut line, &mut word, &mut out);
                }
            }
            flush(&mut line, &mut word, &mut out);
            out.push(line.trim_end().to_string());
        }
        out
    }

    fn wrapped(&mut self, x: f32, y: f32, w: f32, px: f32, rgb: [u8; 3], a: f32, t: &str) -> f32 {
        let lines = self.wrap(t, w, px);
        for (i, l) in lines.iter().enumerate() {
            self.text(x, y + i as f32 * px * 1.3, px, rgb, a, l);
        }
        lines.len() as f32 * px * 1.3
    }

    /// 文字输入框（ImGui 的：能打字、粘贴，Enter / 点别处结束，Esc 取消）；返回正在输入。整个框都能点
    #[allow(clippy::too_many_arguments)]
    fn input(&mut self, id: &str, x: f32, y: f32, w: f32, h: f32, buf: &mut String, hint: &str, focus: bool) -> bool {
        use hudhook::imgui::{StyleColor, StyleVar};
        self.rect(x, y, w, h, [18, 16, 11], 0.85);
        self.sprite("MENU_Line_01", x, y + h - 2.0, w, 2.0, 0.7);
        let ui = self.ui;
        self.scale(22.0);
        let clear = [0.0, 0.0, 0.0, 0.0];
        let _a = ui.push_style_color(StyleColor::FrameBg, clear);
        let _b = ui.push_style_color(StyleColor::FrameBgHovered, clear);
        let _c = ui.push_style_color(StyleColor::FrameBgActive, clear);
        let _d = ui.push_style_color(StyleColor::Text, [TEXT[0] as f32 / 255.0, TEXT[1] as f32 / 255.0, TEXT[2] as f32 / 255.0, 1.0]);
        let _e = ui.push_style_var(StyleVar::FramePadding([12.0 * self.s, (h - 22.0) / 2.0 * self.s]));
        ui.set_cursor_screen_pos(self.p(x, y));
        ui.set_next_item_width(w * self.s);
        if focus {
            ui.set_keyboard_focus_here();
        }
        ui.input_text(id, buf).hint(hint).build();
        ui.is_item_active()
    }

    /// 马上要滚进来的图标先解好
    fn preload(&mut self, id: u16) {
        if id != 0 {
            self.tex.preload_icon(id);
        }
    }

    fn scrollbar(&mut self, x: f32, y: f32, h: f32, top: usize, vis: usize, n: usize) {
        if n <= vis {
            return;
        }
        self.sprite("MENU_FL_scroll_waku", x, y, 14.0, h, 0.9);
        let th = (h * vis as f32 / n as f32).max(30.0);
        let ty = y + (h - th) * top as f32 / (n - vis) as f32;
        self.sprite("MENU_FL_scroll_Bar", x + 3.0, ty, 8.0, th, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 索引：n 件防具（序号 100 起，都能当来源）、m 把武器（序号 500 起）
    fn index(n: u32, m: u32) -> String {
        let mut t = String::from("lang\t0\n");
        for i in 0..n {
            t += &format!("a\t{}\t0\t{}\t0\t0\t1\t{}\t-\n", 100 + i, i % 4, 10000 - i);
        }
        for i in 0..m {
            t += &format!("w\t{}\t0\t0\t0\t3\t1\t{}\t-\n", 500 + i, 10000 - i);
        }
        t
    }

    fn lazy(n: u32, m: u32) -> Workshop {
        let mut w = Workshop::default();
        w.set_snap(Snap::parse(&index(n, m)));
        w
    }

    fn want(w: &mut Workshop) -> String {
        w.request_data();
        w.reqs.drain(..).filter_map(|r| if let Req::Want(t) = r { Some(t) } else { None }).collect()
    }

    /// 要的文字里某一类的序号
    fn ids(text: &str, kind: &str) -> Vec<u32> {
        text.lines().filter_map(|l| l.strip_prefix(kind)).flat_map(|v| v.trim().split(',').filter_map(|x| x.parse().ok()).collect::<Vec<_>>()).collect()
    }

    /// dark_core 给一件防具的明细
    fn detail(i: u32) -> String {
        format!("tgt\t{i}\t0\t{}\t0\t0\t7\tname{i}\naff\t{i}\t0\t1\t0\tA\ttext\ntal\t{i}\t0\t1\t9\tT\n", (i - 100) % 4)
    }

    #[test]
    fn only_the_window_is_requested() {
        let mut w = lazy(100, 0);
        let t = want(&mut w);
        assert_eq!(ids(&t, "a "), (100..100 + (LIST_VIS + LIST_MARGIN) as u32).collect::<Vec<_>>(), "看得见的 9 行 + 后面 6 行");
        assert!(ids(&t, "o ").is_empty());
        assert_eq!(want(&mut w), "", "要过的不再要");
        assert!(w.loading());
    }

    #[test]
    fn scrolling_fetches_only_new_rows() {
        let mut w = lazy(0, 100);
        w.page = 1;
        assert_eq!(ids(&want(&mut w), "w "), (500..515).collect::<Vec<_>>());
        for _ in 0..10 {
            w.wheel(Scroll::Wpn, 1);
        }
        assert_eq!(w.rtop, 30);
        assert_eq!(w.rsel, 0, "滚轮不动选中的行");
        assert_eq!(ids(&want(&mut w), "w "), (524..545).collect::<Vec<_>>(), "第一屏之后的第 30 行，前后各留 6 行");
        w.wheel(Scroll::Wpn, 1);
        assert_eq!(ids(&want(&mut w), "w "), (545..548).collect::<Vec<_>>(), "翻一格只要新进来的");
    }

    #[test]
    fn wheel_stops_at_the_end() {
        let mut w = lazy(0, 12);
        w.page = 1;
        for _ in 0..5 {
            w.wheel(Scroll::Wpn, 1);
        }
        assert_eq!(w.rtop, 3);
        for _ in 0..5 {
            w.wheel(Scroll::Wpn, -1);
        }
        assert_eq!(w.rtop, 0);
    }

    #[test]
    fn list_only_follows_selection_when_told() {
        let mut top = 30;
        fit_top(&mut top, 0, 9, 100, false);
        assert_eq!(top, 30, "鼠标翻的：选中的行在屏幕外也不拉回去");
        fit_top(&mut top, 0, 9, 100, true);
        assert_eq!(top, 0);
        let mut top = 95;
        fit_top(&mut top, 0, 9, 100, false);
        assert_eq!(top, 91, "只收拢到范围里");
    }

    #[test]
    fn inherit_requests_visible_cells_and_options() {
        let mut w = lazy(100, 0);
        let first = want(&mut w);
        let patch: String = ids(&first, "a ").into_iter().map(detail).collect();
        w.merge_patch(&patch);
        let t = want(&mut w);
        let lines: Vec<&str> = t.lines().filter(|l| l.starts_with("o 100 0 ")).collect();
        assert_eq!(lines.len(), 1, "{t}");
        let tgts = ids(lines[0].trim_start_matches("o 100 0"), "");
        assert_eq!(tgts.len(), 5 * GRID_COLS - 1, "看得见的 4 行 + 后面 1 行，第一格是提取成护符");
        assert!(!tgts.contains(&100), "来源自己不算目标");
        assert!(t.lines().any(|l| l.starts_with("o 101 0 ")), "下一件来源的第一条也先要");
        assert!(!t.lines().any(|l| l.starts_with("o 104 ")), "太远的来源不要");
    }

    #[test]
    fn cells_skip_the_source_and_wait_for_options() {
        let mut w = lazy(6, 0);
        let patch: String = (100..106).map(detail).collect();
        w.merge_patch(&patch);
        assert_eq!(w.cells_len(), 6, "提取成护符 + 另外 5 件");
        assert!(matches!(w.cell_at(0), Some(Cell::Tal { ok: true, .. })));
        let idx = |c: Option<Cell>| if let Some(Cell::Tgt { p, .. }) = c { p.idx } else { 0 };
        assert!((1..6).all(|i| idx(w.cell_at(i)) != 100));
        assert!(matches!(w.cell_at(1), Some(Cell::Tgt { known: false, .. })));
        w.merge_patch("tal\t100\t0\t0\t0\tno\n");
        assert_eq!(w.first_ok_cell(), 0, "后面还没加载：先不跳");
        let t = idx(w.cell_at(2));
        let u = idx(w.cell_at(1));
        w.merge_patch(&format!("nopt\t100\t0\t{u}\tnope\nopt\t100\t0\t{t}\t-1\t1\t\n"));
        assert!(matches!(w.cell_at(1), Some(Cell::Tgt { known: true, ok: false, .. })));
        assert_eq!(w.first_ok_cell(), 2);
    }

    #[test]
    fn patches_replace_instead_of_piling_up() {
        let mut w = lazy(3, 0);
        let p = format!("{}opt\t100\t0\t101\t-1\t1\t\n", detail(100));
        w.merge_patch(&p);
        w.merge_patch(&p);
        let s = w.snap.as_ref().unwrap();
        assert_eq!(s.affs[&100].len(), 1);
        assert_eq!(s.opts[&(100, 0, 101)].len(), 1);
    }

    #[test]
    fn details_for_unknown_gear_are_ignored() {
        let mut w = lazy(1, 1);
        w.merge_patch("tgt\t999\t0\t0\t0\t0\t7\tghost\nwpn\t998\t0\t1\t0\t0\t0\t3\tghost\n");
        let s = w.snap.as_ref().unwrap();
        assert_eq!((s.pieces.len(), s.weapons.len()), (1, 1));
    }

    #[test]
    fn full_snapshot_needs_no_requests() {
        let mut w = Workshop::default();
        w.set_snap(Snap::parse("src\t1\t0\t0\t5\tA\ntgt\t1\t0\t0\t0\t0\t5\tA\ntgt\t2\t0\t1\t1\t0\t6\tB\naff\t1\t0\t1\t0\tX\tx\nopt\t1\t0\t2\t-1\t1\t\nwpn\t3\t0\t1\t0\t0\t0\t3\tW\n"));
        assert_eq!(want(&mut w), "");
        assert!(w.src().is_some_and(|p| p.loaded));
        assert_eq!(w.cells_len(), 1);
        assert!(matches!(w.cell_at(0), Some(Cell::Tgt { ok: true, known: true, .. })));
        assert_eq!(w.wview().len(), 1);
    }

    #[test]
    fn hover_needs_a_moved_pointer_and_a_short_rest() {
        let mut w = lazy(6, 0);
        let h = Some(Hit::Src(2));
        w.hover_step(h, false, 1.0);
        assert_eq!(w.sel[0], 0, "列表从不动的指针下面滚过：不算");
        w.hover_step(None, false, 0.0);
        w.hover_step(h, true, 0.05);
        assert_eq!(w.sel[0], 0, "路过的不算");
        w.hover_step(h, false, 0.06);
        assert_eq!(w.sel[0], 2, "停下来了");
    }

    #[test]
    fn hover_only_touches_the_focused_column() {
        let mut w = lazy(6, 0);
        w.col = 1;
        w.hover_step(Some(Hit::Src(2)), true, 1.0);
        assert_eq!(w.sel[0], 0);
    }

    #[test]
    fn worn_first_then_newest() {
        let s = Snap::parse("a\t1\t0\t0\t0\t0\t0\t5\t-\na\t2\t0\t0\t1\t0\t0\t1\t-\na\t3\t0\t0\t0\t0\t0\t9\t-\nw\t7\t0\t0\t0\t3\t0\t2\t-\nw\t8\t0\t1\t0\t3\t0\t1\t-\nw\t9\t0\t0\t0\t3\t0\t6\t-\n");
        let a = |v: &[usize]| v.iter().map(|&i| s.pieces[i].idx).collect::<Vec<_>>();
        let w = |v: &[usize]| v.iter().map(|&i| s.weapons[i].idx).collect::<Vec<_>>();
        assert_eq!(a(&s.tgt_view), vec![2, 3, 1], "穿着的在最前，然后新的在前");
        assert_eq!(a(&s.src_view[0]), vec![3, 1], "穿着的不能当来源");
        assert_eq!(w(&s.wpn_view[0]), vec![8, 9, 7]);
    }

    /// 4 件防具：词条 p q / p / q r s / 没有；1 件穿着（不当来源）；2 把武器：特效 x / 没有
    fn keyed() -> Workshop {
        let mut w = Workshop::default();
        w.set_snap(Snap::parse("a\t1\t0\t0\t0\t0\t1\t4\tp,q\na\t2\t0\t0\t0\t0\t1\t3\tp\na\t3\t0\t0\t0\t0\t1\t2\tq,r,s\na\t4\t0\t0\t0\t0\t1\t1\t-\na\t5\t0\t0\t1\t0\t1\t9\tp\nw\t7\t0\t0\t0\t3\t1\t2\tx\nw\t8\t0\t0\t0\t3\t1\t1\t-\n"));
        w
    }

    fn src_ids(w: &Workshop) -> Vec<u32> {
        let s = w.snap.as_ref().unwrap();
        w.src_view().iter().map(|&i| s.pieces[i].idx).collect()
    }

    #[test]
    fn affix_filter_needs_every_chosen_affix() {
        let mut w = keyed();
        w.open_picker();
        let keys: Vec<String> = w.pick_rows().into_iter().map(|r| r.0).collect();
        assert_eq!(keys, ["p", "q", "r", "s"], "只列这一页身上有的词条");
        let a = Act { a: true, ..Act::default() };
        w.handle(a);
        w.clamp();
        assert_eq!(src_ids(&w), vec![1, 2]);
        w.handle(Act { down: true, ..Act::default() });
        w.handle(a);
        w.clamp();
        assert_eq!(src_ids(&w), vec![1], "选几个就要同时有这几个");
        assert_eq!(w.pick_rows()[2], ("r".to_string(), 0), "后面是选上它之后还剩几件");
    }

    #[test]
    fn affix_filter_cap() {
        let mut w = keyed();
        w.open_picker();
        for i in 0..4 {
            w.picker.as_mut().unwrap().sel = i;
            w.handle(Act { a: true, ..Act::default() });
        }
        assert_eq!(w.kf[0], ["p", "q", "r"], "继承最多 3 个");
        assert!(matches!(&w.msg, Some((false, _))));
        w.handle(Act { x: true, ..Act::default() });
        assert!(w.kf[0].is_empty());
    }

    #[test]
    fn weapon_pages_take_one_affix() {
        let mut w = keyed();
        w.page = 1;
        assert_eq!(w.kf_slot(), Some((1, 1)));
        w.page = 2;
        assert_eq!(w.kf_slot(), Some((2, 1)));
        w.otab = 1;
        assert_eq!(w.kf_slot(), Some((3, 3)));
        w.otab = 0;
        w.open_picker();
        w.handle(Act { a: true, ..Act::default() });
        w.clamp();
        assert_eq!(w.oview().len(), 1);
    }

    #[test]
    fn keyword_asks_for_search_text_once() {
        let mut w = keyed();
        w.request_data();
        w.reqs.clear();
        w.set_query("火 Sword");
        assert!(src_ids(&w).is_empty(), "搜索文字还没来");
        assert_eq!(want(&mut w), "s\n");
        assert!(w.loading());
        assert_eq!(want(&mut w), "");
        w.merge_patch("sa\t1\t火焰 LONG SWORD\nsa\t2\t火焰\n");
        w.clamp();
        assert_eq!(src_ids(&w), vec![1], "几段都要有，不分大小写");
        w.merge_patch("sdone\n");
        w.loading = 0;
        assert!(!w.loading());
    }

    #[test]
    fn changing_filters_goes_back_to_the_top() {
        let mut w = keyed();
        w.sel[0] = 2;
        w.top[0] = 1;
        w.kf[0] = vec!["q".to_string()];
        w.clamp();
        assert_eq!((w.sel[0], w.top[0]), (0, 0));
    }

    #[test]
    fn new_snapshot_keeps_selection_but_drops_search_text() {
        let mut w = keyed();
        w.sel[0] = 2;
        w.search_asked = true;
        w.set_snap(Snap::parse(&index(6, 0)));
        assert_eq!(w.sel[0], 2);
        assert!(!w.search_asked);
    }

    #[test]
    fn pad_walks_up_to_the_filter_button() {
        let act = |f: fn(&mut Act)| {
            let mut a = Act::default();
            f(&mut a);
            a
        };
        let up = act(|a| a.up = true);
        let mut w = keyed();
        w.sel[0] = 1;
        w.handle(up);
        assert_eq!((w.sel[0], w.head), (0, false), "先走到第一行");
        w.handle(up);
        assert!(w.head);
        w.handle(act(|a| a.a = true));
        assert!(w.picker.is_some());
        w.handle(act(|a| a.b = true));
        assert!(w.picker.is_none() && w.head, "关掉窗口回到按钮");
        w.handle(act(|a| a.b = true));
        assert!(!w.head && w.reqs.is_empty(), "B 回列表，不关工坊");
        w.handle(up);
        w.handle(act(|a| a.down = true));
        assert!(!w.head);
    }

    /// 4 件防具：序号、稀有度、减伤率、重量；102 穿着（不当来源，目标里在最前）
    fn sortable() -> Workshop {
        let mut w = Workshop::default();
        let rows = [(100, 1, 0, 20.5, 8.0, 40), (101, 3, 0, 31.0, 3.5, 30), (102, 2, 1, 25.0, 12.0, 20), (103, 1, 0, 18.0, 5.0, 10)];
        let mut s = String::from("lang\t0\n");
        for (idx, rar, worn, cut, wt, sort) in rows {
            s += &format!("a\t{idx}\t{rar}\t0\t{worn}\t0\t0\t{sort}\t-\t{cut}\t{wt}\n");
        }
        w.set_snap(Snap::parse(&s));
        w
    }

    fn tgt_ids(w: &Workshop) -> Vec<u32> {
        let s = w.snap.as_ref().unwrap();
        w.v_tgt.iter().map(|&i| s.pieces[i].idx).collect()
    }

    #[test]
    fn sort_by_negation_and_weight() {
        let mut w = sortable();
        assert_eq!(src_ids(&w), vec![100, 101, 103], "默认：新的在前");
        assert_eq!(tgt_ids(&w), vec![102, 100, 101, 103], "默认：穿着的在前");
        w.pick_sort(2);
        assert_eq!(src_ids(&w), vec![101, 100, 103], "减伤率高的在前");
        assert_eq!(tgt_ids(&w), vec![101, 102, 100, 103], "目标一起排，不再穿着的优先");
        w.toggle_order();
        assert_eq!(src_ids(&w), vec![103, 100, 101], "升序");
        w.pick_sort(3);
        assert_eq!(src_ids(&w), vec![101, 103, 100], "重量升序");
        w.toggle_order();
        assert_eq!(tgt_ids(&w), vec![102, 100, 103, 101], "重量降序");
        w.pick_sort(0);
        assert_eq!(src_ids(&w), vec![100, 101, 103], "回到默认降序");
        w.toggle_order();
        assert_eq!(src_ids(&w), vec![103, 101, 100], "默认升序：旧的在前");
    }

    /// 稀有度：降序传说在前，一样的按默认（新的在前）；升序普通在前；穿着的不再优先
    #[test]
    fn sort_by_rarity() {
        let mut w = sortable();
        w.pick_sort(1);
        assert_eq!(src_ids(&w), vec![101, 100, 103], "传说在前，两件稀有新的在前");
        assert_eq!(tgt_ids(&w), vec![101, 102, 100, 103]);
        w.toggle_order();
        assert_eq!(src_ids(&w), vec![100, 103, 101], "升序：稀有在前，传说最后");
        assert_eq!(tgt_ids(&w), vec![100, 103, 102, 101]);
    }

    #[test]
    fn sort_change_keeps_the_selected_source() {
        let mut w = sortable();
        w.sel[0] = 2; // 103
        w.pick_sort(2);
        assert_eq!(src_ids(&w)[w.sel[0]], 103);
        w.toggle_order();
        assert_eq!(src_ids(&w)[w.sel[0]], 103);
    }

    #[test]
    fn old_index_without_stats_still_sorts() {
        let mut w = keyed();
        let before = src_ids(&w);
        w.pick_sort(3);
        assert_eq!(src_ids(&w), before, "旧的 dark_core 没有减伤率 / 重量：都是 0，按默认顺序");
    }

    #[test]
    fn pad_reaches_the_sort_menu() {
        let act = |f: fn(&mut Act)| {
            let mut a = Act::default();
            f(&mut a);
            a
        };
        let mut w = sortable();
        w.handle(act(|a| a.up = true));
        assert!(w.head && w.head_at == 2, "先落在词条筛选上");
        w.handle(act(|a| a.left = true));
        w.handle(act(|a| a.left = true));
        assert_eq!(w.head_at, 0);
        w.handle(act(|a| a.a = true));
        assert_eq!(w.sort_menu, Some(0));
        for _ in 0..4 {
            w.handle(act(|a| a.down = true)); // 第四下到底了，停在重量
        }
        w.handle(act(|a| a.a = true));
        assert_eq!((w.sort, w.sort_menu), (3, None), "选了重量，菜单关上");
        assert!(w.head, "还在标题栏");
        w.handle(act(|a| a.right = true));
        w.handle(act(|a| a.a = true));
        assert!(w.asc);
        w.handle(act(|a| a.x = true));
        assert_eq!((w.sort, w.asc), (0, false), "X 恢复默认");
        w.handle(act(|a| a.b = true));
        assert!(!w.head && w.reqs.is_empty());
    }

    #[test]
    fn sort_menu_closes_on_other_pages() {
        let mut w = sortable();
        w.toggle_sort_menu();
        assert!(w.sort_menu.is_some());
        w.handle(Act { b: true, ..Act::default() });
        assert!(w.sort_menu.is_none() && w.reqs.is_empty(), "B 只关菜单");
        w.page = 1;
        w.toggle_sort_menu();
        assert!(w.sort_menu.is_none(), "重铸页没有排序");
    }

    #[test]
    fn pages_without_a_filter_have_no_button() {
        let mut w = keyed();
        w.page = 5;
        w.handle(Act { up: true, ..Act::default() });
        assert!(!w.head);
    }

    #[test]
    fn keyword_matches_pinyin() {
        let mut w = keyed();
        w.merge_patch("sa\t1\t黑骑士头盔 凝聚\x01施放魔法\nsa\t2\t长剑\x01凝聚法球\nsdone\n");
        for (q, ids) in [("ningju", vec![1, 2]), ("nj", vec![1]), ("hqs", vec![1]), ("qiu", vec![2])] {
            w.set_query(q);
            w.clamp();
            assert_eq!(src_ids(&w), ids, "{q}");
        }
    }

    #[test]
    fn guide_icons_map_to_buttons() {
        assert!(glyph_act("KG_XboxSeries_R_R").b);
        assert!(glyph_act("KG_XboxSeries_R2").rt);
        assert_eq!(glyph_act("nothing"), Act::default());
        assert!(glyph_act("KG_XboxSeries_R_U").y);
    }

    fn reforges(w: &mut Workshop) -> Vec<u32> {
        w.reqs.drain(..).filter_map(|r| if let Req::Reforge(k) = r { Some(k) } else { None }).collect()
    }

    #[test]
    fn quick_reforge_skips_the_dialog() {
        let mut w = keyed();
        w.page = 1;
        w.snap.as_mut().unwrap().rstones = 2;
        let y = Act { y: true, ..Act::default() };
        w.handle(y);
        assert!(!w.confirm, "Y 不弹确认");
        assert_eq!(reforges(&mut w), vec![7], "直接重铸选中的那把");
        w.handle(y);
        assert!(reforges(&mut w).is_empty(), "上一次还没回结果，不再发");
        w.done(true, "ok".to_string());
        w.handle(y);
        assert_eq!(reforges(&mut w), vec![7], "回了结果可以接着重铸");
        w.done(true, "ok".to_string());
        // A 照旧：到右边按钮、再 A 弹确认，不直接重铸
        let a = Act { a: true, ..Act::default() };
        w.rcol = 0;
        w.handle(a);
        w.handle(a);
        assert!(w.confirm);
        assert!(reforges(&mut w).is_empty());
    }

    #[test]
    fn confirm_waits_before_taking_a_press() {
        let mut w = keyed();
        w.page = 1;
        w.snap.as_mut().unwrap().rstones = 2;
        let a = Act { a: true, ..Act::default() };
        w.handle(a);
        w.handle(a);
        assert!(w.confirm, "A、A 弹确认");
        w.handle(a);
        w.handle(Act { y: true, ..Act::default() });
        assert!(w.confirm && reforges(&mut w).is_empty(), "刚弹出来接着按的不算确定");
        w.confirm_t = CONFIRM_GUARD;
        w.handle(a);
        assert!(!w.confirm);
        assert_eq!(reforges(&mut w), vec![7], "等一下再按才确定");
        w.done(true, "ok".to_string());
        w.handle(a);
        assert!(w.confirm);
        w.handle(Act { b: true, ..Act::default() });
        assert!(!w.confirm, "取消不用等");
    }

    #[test]
    fn quick_reforge_refuses_without_stones_or_after_overlimit() {
        let mut w = keyed();
        w.page = 1;
        let y = Act { y: true, ..Act::default() };
        w.handle(y);
        assert!(reforges(&mut w).is_empty(), "没有重铸石");
        assert!(matches!(&w.msg, Some((false, _))));
        w.snap.as_mut().unwrap().rstones = 5;
        let i = w.wview()[0];
        w.snap.as_mut().unwrap().weapons[i].ovl = true;
        w.handle(y);
        assert!(reforges(&mut w).is_empty(), "超限过的不能重铸");
        assert!(!w.confirm && !w.waiting);
    }
}
