//! 新装备系统（阶段 1，dev\item_system\设计文档.md 第 8 节）：赐福「一键鉴定」把背包里的未鉴定道具换成按词条库现抽的装备。
//! 重随那边在 DLL 旁边写两个定义文件（没有就什么都不做）：
//!   items.txt    容器（预留的空白参数行）、未鉴定道具、发货表、标记（rr/containers.py）
//!   affixes.txt  词条库、能当本体的原版武器 / 防具、要改的字段偏移（rr/affixes.py）
//! 流程：
//!   1. 赐福菜单「一键鉴定」点亮请求标记（sc/identify.py）；这里看到（人物还要正坐在赐福点）就熄灭它，数背包里的未鉴定道具；
//!   2. 每件挑一个这个角色没在用的容器，按词条库抽（本体、补正、异常、词条……），改写容器的参数行（照本体整行复制，
//!      跟编号绑定的字段换回容器自己的）和三段文字（游戏取文字时由 msghook.rs 的钩子换成我们的，不写游戏内存）；
//!   3. 这批容器写进发货表、点亮发货标记，公共事件发完熄灭；再等 1 秒数背包 + 仓库，到了的才记进记录、扣掉对应的未鉴定道具
//!      （游戏自己的移除函数，同 dark_sell）；一批最多发货表的行数，还有剩的接着发下一批。
//! 记录：%APPDATA%\EldenRing\DarkReforged\角色记录\items_<格子>.txt（第一行角色名），一行一个容器的配方；进存档时按记录重建所有容器的行和文字。
//!   （2026-10-29 以前在 EldenRing 根目录的 DarkReforged_items_<格子>.txt：启动器会搬过来，新位置没有时这里也读旧的，见 rec_read。）
//!   容器只要不在背包 / 仓库里（卖了、丢了）就算空的，下次鉴定可以再用（不同角色各用各的，读档时按这个角色的记录改写）。
//! 确认框里的「背包里几件、容器还剩多少」：对话脚本留的占位「〔#I0 …〕」（英文 [ ]），每 30 帧按背包现状算好交给取文字钩子。
//! 定向鉴定（用户 2026-11-03）：赐福「暗黑重铸」→「鉴定」里选武器类型、稀有度，菜单把类型序号、稀有度写进 items.txt dirident 行的标记再点亮请求；
//! 这里只鉴定那个稀有度的未鉴定武器（全部 = 5），本体从那个类型里挑（同样偏向加点），稀有度不变，每件按它的稀有度扣卢恩（cost，
//! 用户 2026-11-03：普通 10、稀有 100、史诗 500、传说 1000、传说（幽影之地）2000），付不起的不鉴定；
//! 一件都鉴定不了（没有这种未鉴定武器、卢恩不够、容器满）就点亮「没鉴定」标记，菜单提示。确认框占位 #I7 写各稀有度的件数和卢恩够几件。
//! 防具鉴定、护符鉴定（用户 2026-11-09「新加防具鉴定和护符鉴定 防具鉴定可以定向部位 护符鉴定可以定向流派 护符鉴定价格是其他的10倍」）：
//!   dirident 行多了种类标记（0 武器 / 1 防具 / 2 护符）、护符每件的卢恩、流派名；防具的序号就是部位（0 头 / 1 身 / 2 手 / 3 腿），
//!   护符的序号是 dirident 列的流派（fam 行的流派）。确认框占位 #I9 写未鉴定防具、#IA 写未鉴定护符各稀有度的件数。
//! 回购商店（items.txt 的 buyback 行，暗黑商店里的一个子商店）：卖掉 / 丢掉的生成装备挪进记录的 sold 段（最后 n 件），
//!   容器留着不给鉴定用；商店第 k 行改成第 k 件、点亮它的上架标记；买回来（又出现在背包里）就挪回记录。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use eldenring::cs::{
    AttackElementCorrectParam, CSEventFlagMan, CalcCorrectGraph, ChrInsExt, EquipInventoryData, EquipParamAccessory, EquipParamGoods,
    EquipParamProtector, EquipParamWeapon, GameDataMan, ReinforceParamWeapon,
    ItemLotParam_map, PlayerGameData, ShopLineupParam, SoloParam, SoloParamRepository, WorldChrMan,
};
use fromsoftware_shared::FromStatic;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::{dir, log};

mod ashes;
mod batch;
mod deathloss;
mod devgive;
mod gift;
mod overlimit;
mod valfx;
mod workshop;

use batch::{Bias, Made, Src, Want};

pub(crate) use workshop::graph;

/// 三类：武器 / 防具 / 护符
const NCAT: usize = 3;
const CAT_CODE: [char; NCAT] = ['w', 'p', 'a'];
const CAT_ZH: [&str; NCAT] = ["武器", "防具", "护符"];
/// 背包里物品编号的类别
const INV_CAT: [u8; NCAT] = [0, 1, 2];
const INV_GOODS: u8 = 4;
/// 掉落表的类别（道具 1）
const LOT_CAT: [i32; NCAT] = [2, 3, 4];
const FIELDS: [char; 3] = ['N', 'I', 'C'];
const REC_PREFIX: &str = "items_";
/// 进存档后多少帧内亮着的鉴定请求算存档里残留的
const SETTLE_FRAMES: u32 = 300;
const STATUS_N: usize = 7;
const CURVE_N: usize = 9;
/// 商店的库存计数（eventFlag_forStock 是「个数保持」的多位计数）最多占几位：原版每格都隔 10 个号
const STOCK_BITS: u32 = 10;

// ------------------------------------------------------------------ 定义
/// 按权重抽的分布（值, 权重）
#[derive(Debug, Default, Clone, PartialEq)]
struct Dist {
    items: Vec<(f64, u32)>,
    total: u64,
}

impl Dist {
    fn parse(fields: &[&str]) -> Option<Dist> {
        let mut d = Dist::default();
        for f in fields {
            let (v, w) = f.split_once(':')?;
            let (v, w): (f64, u32) = (v.parse().ok()?, w.parse().ok()?);
            if w > 0 {
                d.items.push((v, w));
                d.total += w as u64;
            }
        }
        Some(d)
    }

    fn pick(&self, rng: &mut Rng) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        let mut x = rng.below(self.total);
        for &(v, w) in &self.items {
            if x < w as u64 {
                return Some(v);
            }
            x -= w as u64;
        }
        self.items.last().map(|i| i.0)
    }

    /// 值最高的那部分：从大到小累计权重到 frac 为止（跨线的那个值整个算进来）
    fn top(&self, frac: f64) -> Dist {
        if frac >= 1.0 {
            return self.clone();
        }
        let mut v = self.items.clone();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        let need = ((self.total as f64 * frac).ceil() as u64).max(1);
        let mut d = Dist::default();
        for (x, w) in v {
            if d.total >= need {
                break;
            }
            d.items.push((x, w));
            d.total += w as u64;
        }
        d
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Field {
    off: usize,
    fmt: u8,
    /// 位域的第几位；-1 = 整个字段
    bit: i32,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Cont {
    first: u32,
    step: u32,
    count: u32,
    size: usize,
    keep: Vec<(usize, usize)>,
    /// 第二段：(第一个编号, 从第几个起)。武器 28 位编号第一段放不下时用（用户 2026-10-30 扩到 3000，rr/containers.py）；None = 只有一段
    second: Option<(u32, u32)>,
}

impl Cont {
    fn id(&self, idx: u32) -> u32 {
        match self.second {
            Some((first2, split)) if idx >= split => first2 + (idx - split) * self.step,
            _ => self.first + idx * self.step,
        }
    }

    fn idx_of(&self, id: u32) -> Option<u32> {
        let seg = |first: u32, lo: u32, n: u32| {
            (id >= first && (id - first) % self.step == 0).then(|| (id - first) / self.step).filter(|&k| k < n).map(|k| k + lo)
        };
        match self.second {
            Some((first2, split)) => seg(self.first, 0, split.min(self.count)).or_else(|| seg(first2, split, self.count.saturating_sub(split))),
            None => seg(self.first, 0, self.count),
        }
    }
}

#[derive(Debug, Default)]
struct Items {
    cont: [Cont; NCAT],
    /// 占位长度：名字 / 简介 / 说明
    cap: [usize; 3],
    /// 占位后面的说明（中文, 英文）
    label: [String; 2],
    /// 道具编号 → (类, 档 0~4)
    goods: HashMap<u32, (usize, usize)>,
    deliver_lot: u32,
    deliver_rows: u32,
    deliver_flag: u32,
    identify_flag: u32,
    lot_size: usize,
    /// 物品编号、类别、权重、个数、获取标记的偏移
    lot_off: [usize; 5],
    /// 开局装备：一启动就按种子生成（新角色拿到的就是这一份）；角色进存档时记进这个角色的记录，以后按记录（一次性重随不影响已有角色）
    start: Vec<StartSpec>,
    /// 开局装备的编号段（武器 / 防具）：(第一个编号, 间隔)，配方序号 = START_BIT | (编号 - 第一个) / 间隔
    start_ids: [(u32, u32); 2],
    /// 向星辰许愿（rr/wish.py）；没有 wish 行就没有
    wish: Option<Wish>,
    /// 回购商店（rr/darkshop.py）；没有 buyback 行就没有
    buyback: Option<Buyback>,
    /// 暗黑商店的武器商店（rr/darkshop.py）：(商店行, 武器类型 wepType, 档 0~2)
    wshop: Vec<(u32, u16, u8)>,
    /// 武器重铸石（rr/reforge.py）；没有 reforge 行就没有
    reforge: Option<Reforge>,
    /// 暗黑商店的装备回收（同上）；没有 recycle 行就没有
    recycle: Option<Recycle>,
    /// 赐福「装备继承」（rr/inherit.py）；没有 inherit 行就没有
    inherit: Option<Inherit>,
    /// 超限锻造石（rr/overlimit.py）；没有 overlimit 行就没有
    overlimit: Option<Overlimit>,
    /// 定向鉴定（rr/containers.py、sc/identify.py）；没有 dirident 行就没有
    dirid: Option<DirId>,
    /// 掉落池（pool 行，gear/batch.rs）：鉴定 / 许愿做某类某档时按几率换成固定物品
    pools: Vec<batch::Pool>,
    /// 死亡惩罚（deathloss 行，gear/deathloss.rs）：真死了扣掉背包里未鉴定道具的百分比；0 = 不扣
    deathloss: f64,
    /// 遗失的未鉴定物品直接放回背包、不弹框（deathback 行，gear/deathloss.rs、sc/identify.py）；没有这行（老的重随包）照旧走发货表
    deathback: Option<deathloss::Back>,
}

/// 定向鉴定：请求标记、类型序号写进的标记（第一个、几位）、稀有度写进的标记（第一个、几位；5 = 全部）、
/// 一件都没鉴定的标记、每件的卢恩（按档 0~4）、类型序号 → 武器类型 wepType
#[derive(Debug, Default, Clone, PartialEq)]
struct DirId {
    flag: u32,
    type0: (u32, u32),
    rar0: (u32, u32),
    none: u32,
    cost: [u32; 5],
    types: Vec<u16>,
    /// 种类（0 武器 / 1 防具 / 2 护符）写进的标记（第一个、几位；(0, 0) = 只有武器，旧的 items.txt）
    kind0: (u32, u32),
    /// 护符鉴定每件的卢恩（按档；用户 2026-11-09：武器、防具的 10 倍）
    cost_t: [u32; 5],
    /// 护符鉴定的流派序号 → 流派名（fam 行的流派）
    archs: Vec<String>,
}

impl DirId {
    /// 这一类（0 武器 / 1 防具 / 2 护符）这一档的未鉴定物品每件多少卢恩
    fn cost_of(&self, cat: usize, tier: usize) -> u32 {
        if cat == 2 { self.cost_t[tier.min(4)] } else { self.cost[tier.min(4)] }
    }
}

/// 防具鉴定的部位数（0 头 / 1 身 / 2 手 / 3 腿）
const DIR_SLOTS: usize = 4;
const DIR_CAT_ZH: [&str; 3] = ["武器", "防具", "护符"];

/// 一次定向鉴定：类、偏向（武器类型 / 防具部位 / 护符流派）、稀有度（5 = 全部）
#[derive(Debug, Clone, Copy, PartialEq)]
struct DirReq {
    cat: usize,
    bias: Bias,
    rar: usize,
}

/// 赐福「装备工坊」（rr/inherit.py、sc/inherit.py；界面 dll\dark_ui，接口 gear\workshop.rs）：打开请求 open（菜单点亮，关窗口时 DLL 熄灭）、
/// 应答 ack（DLL 看到请求、界面在，点亮）、没有界面 noui（DLL 点亮，菜单提示没加载 dark_ui）、继承消耗的道具 goods（融锻之心；0 = 没有）、
/// 「魇印」「七咒」入口的打开请求 pages（赐福「暗黑重铸」里各自一项，sc/wspages.py；应答 / 没有界面和工坊共用；0 = 没有这个入口）
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Inherit {
    open: u32,
    ack: u32,
    noui: u32,
    goods: u32,
    pages: [u32; 2],
}

/// 武器重铸石：道具、使用时挂的标记效果、重铸成功时挂的金光效果
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Reforge {
    goods: u32,
    mark: i32,
    fx: i32,
}

/// 超限锻造石：武器石、防具石、从中位到超限上限几颗（步长 = （超限上限 − 中位）÷ steps，各稀有度的上限在 Lib 的 ovlw / ovlp）
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Overlimit {
    gw: u32,
    ga: u32,
    /// 从中位到超限上限几颗：武器 / 防具（防具没写就同武器；用户 2026-11-04 防具减半）
    steps: u32,
    asteps: u32,
    /// 法杖、圣印记以外的武器每把的超限上限 × 1 ± 这个数的截断正态（用户 2026-11-21「其他武器加一个上限的随机范围 为正负20%的截断正态」）；
    /// 没写（老的重随包）是 0 = 不随机
    wspread: f32,
}

impl Overlimit {
    /// 这件装备从中位到超限上限几颗
    fn steps_for(&self, rec: &Rec) -> u32 {
        if matches!(rec.recipe, Recipe::P { .. }) { self.asteps } else { self.steps }
    }
}

/// 装备回收：给的道具（重铸石）、请求标记（回收 普通 / 稀有及以下 / 史诗及以下 / 传说及以下）、每把给几个（普通 / 稀有 / 史诗 / 传说）、
/// 回收全部弃置的请求标记（items.txt 的 recycle 行第 4 个数，0 = 没有这个功能；用户 2026-11-26）
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Recycle {
    goods: u32,
    flags: [u32; 4],
    stones: [u32; 4],
    discard: u32,
}

/// 回购商店：商店行 row0 起 n 行；第 k 行的上架标记 rel0 + k、库存计数标记 stock0 + k（计数可能占到 STOCK_BITS 位，整段一起清）
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Buyback {
    row0: u32,
    n: u32,
    rel0: u32,
    stock0: u32,
}

/// 向星辰许愿：请求标记、免费次数、类别权重、各档价格和稀有度、地图 → 档
#[derive(Debug, Default, Clone)]
struct Wish {
    flag1: u32,
    flag10: u32,
    /// 「诚意不够」应答（一次都付不起时点亮，赐福菜单提示；0 = 没有，旧的 items.txt）
    poor: u32,
    free0: u32,
    free_n: u32,
    share: [u32; NCAT],
    bands: Vec<WishBand>,
    /// (区域, 块, 行, 列/层) → 档
    maps: HashMap<(u8, u8, u8, u8), usize>,
}

#[derive(Debug, Default, Clone, Copy)]
struct WishBand {
    cost: u32,
    dlc: bool,
    w: [u32; 4],
}

impl Wish {
    /// 人物所在地图的档：先找一样的，大地图再找上一级 / 上两级图块（_01 / _02），地下城找同一张地图；都没有就 None
    fn band_at(&self, a: u8, b: u8, r: u8, i: u8) -> Option<usize> {
        let m = &self.maps;
        m.get(&(a, b, r, i))
            .or_else(|| m.get(&(a, b, r, 0)))
            .or_else(|| if a >= 60 { m.get(&(a, b / 2, r / 2, 1)).or_else(|| m.get(&(a, b / 4, r / 4, 2))) } else { None })
            .or_else(|| if a < 60 { m.iter().find(|(k, _)| k.0 == a && k.1 == b).map(|(_, v)| v) } else { None })
            .copied()
            .filter(|&k| k < self.bands.len())
    }
}

/// items.txt 的 start 行（重随的 rr/startgear.py）
#[derive(Debug, Clone, PartialEq)]
struct StartSpec {
    cat: usize,
    id: u32,
    tier: u8,
    base: u32,
    seed: u64,
    /// 能力需求上限（职业的初始力气、灵巧、智力、信仰、感应）
    maxreq: [u8; 5],
    /// 武器：补正只从分布里最高的这部分抽（0~1，1 = 不限）、至少几种命中异常（负数 = 一个都不带：开局的法杖、圣印记、弓、盾）
    top: f64,
    min_status: i32,
    /// 武器保留本体的战技和常驻效果（追忆换的成品传说：就是原来那把）
    keep: bool,
    /// 武器的主补正（最高的那项）放在哪项属性上（0~4 力 / 技 / 智 / 信 / 感，职业最高的那项；-1 = 不管）
    main: i32,
    /// 人物拿到时按那时的属性重新生成（大蛇狩猎矛，rr/serpent.py）：主补正放在最高的属性上、能力需求不超过那时的属性
    pick: bool,
    /// 武器：不保留本体战技也照原版比例抽全能（商人卖的小极品传说武器）；防具：强制三个词条位（英雄防具商店）
    full: bool,
}

/// 开局武器 / 追忆武器的约束：本体固定、能力需求截到上限、补正从分布最高的 top 里抽、至少 min_status 种异常、
/// keep = 战技和常驻效果照本体（配方的 skill = -1、res = KEEP_RES）
#[derive(Debug, Clone, Copy)]
struct WFix {
    base: u32,
    maxreq: [u8; 5],
    top: f64,
    min_status: i32,
    keep: bool,
    main: i32,
    full: bool,
}

impl StartSpec {
    fn wfix(&self) -> WFix {
        WFix { base: self.base, maxreq: self.maxreq, top: self.top, min_status: self.min_status, keep: self.keep, main: self.main, full: self.full }
    }
}

/// 配方的常驻效果写这个 = 照本体（不改 residentSpEffectId / 1；传说光效只在本体的 residentSpEffectId2 空着时加）
const KEEP_RES: i32 = -2;

/// 开局装备的配方序号带这一位（序号 = 编号在开局装备编号段里的位置），和容器序号分开；按编号算，一次性重随后件数变了也对得上
const START_BIT: u32 = 0x8000_0000;

/// 开局装备编号 → 配方序号（不在编号段里就 None）
fn start_off(items: &Items, cat: usize, id: u32) -> Option<u32> {
    let (first, step) = *items.start_ids.get(cat)?;
    (step > 0 && id >= first && (id - first) % step == 0 && (id - first) / step < 0x10000).then(|| START_BIT | (id - first) / step)
}

/// 这件装备的参数行编号：容器按序号算，开局装备按编号段算
fn rec_id(items: &Items, rec: &Rec) -> u32 {
    if rec.idx & START_BIT != 0 {
        let (first, step) = items.start_ids.get(rec.cat).copied().unwrap_or((0, 0));
        first + (rec.idx & !START_BIT) * step
    } else {
        items.cont[rec.cat].id(rec.idx)
    }
}

#[derive(Debug, Clone)]
struct WBase {
    typ: u16,
    id: u32,
    var: i32,
    elem: usize,
    name: [String; 2],
}

#[derive(Debug, Clone)]
struct PBase {
    slot: u8,
    id: u32,
    name: [String; 2],
}

#[derive(Debug, Clone)]
struct FamR {
    sp: i32,
    tal: i32,
    text: [String; 2],
    /// 词条数值随机（用户 2026-11-15，affixes.txt 的 famval 等行，rr/affixes.py val_lines）：这一档的范围、说明模板、占位行要写的东西；None = 固定数值
    val: Option<Box<Val>>,
}

/// 一条随机词条一个稀有度的取值定义。装备记录里存的是「在这档范围里的位置」p（-1 = 下端、0 = 现在的数、1 = 上端，存成千分之几），
/// 数值按现在的定义从位置算（调了数值装备跟着变）；说明数字里变化的数：一个主数（按范围抽），其余跟着主数按四档的点插值
#[derive(Debug, Clone, Default)]
pub(crate) struct Val {
    /// 主数的范围、截断正态的 μ、σ（均值正好是现在的数）、现在的数
    lo: f32,
    hi: f32,
    mu: f32,
    sigma: f32,
    c: f32,
    /// 主数在说明数字里的位置、显示精度
    main: usize,
    prec: f32,
    /// 跟着主数插值的数：(位置, 显示精度, 四档的 (主数, 这个数)，按主数排好)
    others: Vec<(usize, f32, Vec<(f32, f32)>)>,
    /// 说明模板（{位置} 换成数）、词条百科的范围
    tpl: [String; 2],
    span: [String; 2],
    /// 效果链（第 0 个是链头）、链内引用 (行, 字段偏移, 指向第几行)、要写的字段 (行, 字段偏移, 类型, a, b, 数的位置)：字段 = a + b × 数
    pub(crate) rows: Vec<i32>,
    pub(crate) refs: Vec<(usize, usize, usize)>,
    pub(crate) fields: Vec<(usize, usize, u8, f32, f32, usize)>,
    /// 二期（积木规则 / 专用模块的词条，装备行上照旧挂稀有度的标记）：
    /// 规则里这一档的数 (这条词条的第几条规则, 字段, a, b, 数的位置)（rules.rs 覆盖规则）；
    /// 规则挂的这一档的效果行 (效果编号, [(偏移, 类型, a, b, 数的位置)])（valfx 原地改写，摘下还原）；专用模块的数 (模块, a, b, 数的位置)（runtime.rs）
    pub(crate) rnum: Vec<(usize, String, f32, f32, usize)>,
    pub(crate) slots: Vec<(i32, Vec<(usize, u8, f32, f32, usize)>)>,
    pub(crate) runs: Vec<(String, f32, f32, usize)>,
}

impl Val {
    /// 链头：链里没被别的行引用的行（一般只有第 0 行；霜铠两个监听各挂一个增益，有两个链头，用户 2026-11-19「可以同时触发两个图标吗」）
    pub(crate) fn roots(&self) -> Vec<usize> {
        let r: Vec<usize> = (0..self.rows.len()).filter(|k| !self.refs.iter().any(|x| x.2 == *k)).collect();
        if r.is_empty() && !self.rows.is_empty() { vec![0] } else { r }
    }
}

fn round_to(v: f32, prec: f32) -> f32 {
    if prec > 0.0 { (v / prec).round() * prec } else { v }
}

/// 说明里的数：整数不带小数点，其余一位小数
fn num_text(v: f32) -> String {
    let v = v.abs();
    if (v - v.round()).abs() < 1e-4 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

/// 按点 (x, y)（x 从小到大）分段插值，两头按端上那段外推
fn interp(pts: &[(f32, f32)], x: f32) -> f32 {
    match pts.len() {
        0 => 0.0,
        1 => pts[0].1,
        n => {
            let k = if x <= pts[0].0 { 0 } else if x >= pts[n - 1].0 { n - 2 } else { (0..n - 1).find(|&k| pts[k].0 <= x && x <= pts[k + 1].0).unwrap_or(0) };
            let ((x0, y0), (x1, y1)) = (pts[k], pts[k + 1]);
            if (x1 - x0).abs() < 1e-9 { y0 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) }
        }
    }
}

impl Val {
    /// 位置 p 的主数（按显示精度取整）
    fn main_at(&self, p: f32) -> f32 {
        let p = p.clamp(-1.0, 1.0);
        round_to(if p >= 0.0 { self.c + p * (self.hi - self.c) } else { self.c + p * (self.c - self.lo) }, self.prec)
    }

    /// 位置 p 的各个变化的数：(在说明数字里的位置, 数)
    pub(crate) fn nums_at(&self, p: f32) -> Vec<(usize, f32)> {
        let m = self.main_at(p);
        let mut out = vec![(self.main, m)];
        for (j, prec, pts) in &self.others {
            // 现在的数正好落在四档的点上：插值就是原来的数
            out.push((*j, round_to(interp(pts, m), *prec)));
        }
        out
    }

    /// 主数 v 在范围里的位置（千分之几）
    fn pos_of(&self, v: f32) -> i16 {
        let p = if v >= self.c { if self.hi > self.c { (v - self.c) / (self.hi - self.c) } else { 0.0 } } else if self.c > self.lo { (v - self.c) / (self.c - self.lo) } else { 0.0 };
        (p.clamp(-1.0, 1.0) * 1000.0).round() as i16
    }

    /// 新装备抽一个位置：截断正态（拒绝采样，64 次都落在外面就用现在的数），按显示精度取整后换成位置
    fn sample(&self, rng: &mut Rng) -> i16 {
        for _ in 0..64 {
            let u1 = ((rng.next() >> 11) as f64 + 1.0) / 9007199254740993.0;
            let u2 = (rng.next() >> 11) as f64 / 9007199254740992.0;
            let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            let v = self.mu as f64 + self.sigma as f64 * z;
            if v >= self.lo as f64 && v <= self.hi as f64 {
                return self.pos_of(round_to(v as f32, self.prec));
            }
        }
        0
    }

    /// 位置 p 的说明（模板里的 {位置} 换成数）
    fn text(&self, p: f32, lang: usize) -> String {
        let mut t = self.tpl[lang].clone();
        for (j, v) in self.nums_at(p) {
            t = t.replace(&format!("{{{j}}}"), &num_text(v));
        }
        t
    }
}

/// 记录里存的位置（千分之几）→ -1~1
fn pos_f(p: i16) -> f32 {
    p as f32 / 1000.0
}

#[derive(Debug, Clone)]
struct Fam {
    key: String,
    /// 抽词条的权重，按稀有度：防具池、护符池（fam 行的权重四档都一样；famw 行按稀有度覆盖，见 extract_affixes.py merge_and_balance）
    armor_w: [u32; 4],
    tal_w: [u32; 4],
    /// 后缀（中文, 英文）
    suffix: [String; 2],
    r: [Option<FamR>; 4],
    /// 能出的防具部位（位：头 1 / 身 2 / 手 4 / 腿 8）；fam 行没写就是全部
    slots: u8,
    /// 机制类（触发、规则改变）：防具名字里做前缀；其余是数值类，做后缀
    mech: bool,
    /// 合并组（变体组的键；空 = 没有）：同一件防具不出同组的两个
    group: String,
}

#[derive(Debug, Default, Clone)]
struct Res {
    types: Vec<u16>,
    n: [Dist; 4],
    /// (效果, 权重, [中文, 英文])
    pool: Vec<(i32, u32, [String; 2])>,
}

/// 武器的一种特殊效果（wspec 行）：键、效果（武器常驻）、权重、能出的武器类型（空 = 全部）、短名、说明
#[derive(Debug, Clone)]
struct WSpec {
    key: String,
    sp: i32,
    w: u32,
    types: Vec<u16>,
    name: [String; 2],
    text: [String; 2],
    /// 能出的稀有度（位：普通 1 / 稀有 2 / 史诗 4 / 传说 8）；wspec 行没写就是全部
    rar: u8,
}

#[derive(Debug, Clone)]
struct SkillInfo {
    gem: bool,
    native: i32,
    name: [String; 2],
}

#[derive(Debug, Default)]
struct Lib {
    rar: [[String; 2]; 4],
    /// 卖价 [类][稀有度]
    sell: [[i32; 4]; NCAT],
    stat: Vec<[String; 2]>,
    fields: HashMap<(char, String), Field>,
    wbase: Vec<WBase>,
    pbase: Vec<PBase>,
    wtype: [Dist; 4],
    wscal_n: [Dist; 4],
    wscal_v: HashMap<(usize, u32), Dist>,
    wreq: HashMap<(usize, usize), Dist>,
    /// 需求和补正排序相同（补正最高的那项需求也最高）
    wreq_sorted: bool,
    /// 各稀有度最低等级（从全 10 起只加要的属性，力气按单手）；0 = 不限
    wreq_minlvl: [u32; 4],
    /// 生成的全能（五项补正）不带命中异常；重铸成全能的保留原有异常（reforge_recipe）
    full_no_status: bool,
    /// 鉴定偏向加点：每件武器生成几把候选（≤1 = 不偏向）
    bias_n: u32,
    wcurve: HashMap<(usize, usize), Dist>,
    /// 按武器类型的补正曲线（wcurvet 行，用户 2026-11-25「照原版重做」）：类型 → [(这一组伤害种类, 曲线分布)]，
    /// 一组共用一次抽到的曲线；有的种类照它，没有的照旧按稀有度的 wcurve（draw_curves）
    wcurvet: HashMap<u16, Vec<(Vec<usize>, Dist)>>,
    wstat_n: [Dist; 4],
    wstat_pow: HashMap<(usize, usize), Dist>,
    wstat_sp: Vec<HashMap<u32, i32>>,
    /// 武器的特殊效果词条位：各稀有度出几个（0 / 1）、特效池
    wspec_n: [Dist; 4],
    wspec: Vec<WSpec>,
    /// 强攻（本体层，不占特效位）：各稀有度出的几率（%）、基础攻击力加成（%）
    wassault: [(u32, u8); 4],
    /// 致命一击按稀有度乘（wcrit 行，crit_by_rarity）：面板的致命一击（100 + throwAtkRate）整个乘这个数；0 = 没写，照原版
    wcrit: [f64; 4],
    /// 生成武器的基础攻击倍率（light_weapon_atk / heavy_weapon_atk）：武器类型 wepType → 五项基础攻击力的倍率（和强攻相乘）；没有就照原版
    wheavy: HashMap<u16, f64>,
    /// 词条位置（各档 0~4 有词条的几率 %）：防具 [前缀（机制类）, 后缀 1, 后缀 2（数值类）]、武器前缀（特效）；
    /// 没有就照旧用 pcount / wspec_n（原包的分布）
    pslot_p: [Option<[u32; 3]>; 5],
    wspec_p: [Option<u32>; 5],
    /// 每种异常在面板「附加效果」里显示的文字（WeaponEffect，如累积出血量表）；没有就 -1
    wstat_msg: [i32; STATUS_N],
    element_table: i32,
    /// 武器属性随机（welem 行；没有这一行就不抽，配方 elem 一律 0 = 照本体）
    welem: WElem,
    glow: [i32; 5],
    res: Vec<(String, Res)>,
    skill: HashMap<u16, Dist>,
    skill_def: HashMap<u16, Dist>,
    skillinfo: HashMap<i32, SkillInfo>,
    /// 专属战技 → (原武器的行为变体, 去掉覆盖通用普攻判定的新变体)（skill_var）
    skillvar: HashMap<i32, (i32, i32)>,
    pslot: [Dist; 4],
    pcount: [Dist; 5],
    fams: Vec<Fam>,
    fam_by_key: HashMap<String, usize>,
    /// 同名只认身上最好的那件的词条（famtop 行，用户 2026-11-24：蓄力攻击伤害、双手持握伤害）：gear/valfx.rs 只给最好的那件挂
    top_only: HashSet<String>,
    /// 词条族的流派（fam 行第 10 列，按出现顺序编号进 arch_names）：护符鉴定按流派挑（用户 2026-11-09）
    fam_arch: Vec<u8>,
    arch_names: Vec<String>,
    /// 强化（重随的 rr/upgrade.py）：(本体的原版强化类型, 稀有度, 全能) → 换成的强化类型；各稀有度的材料组；最高等级（0 = 没有，照原版）
    reinf: HashMap<(u16, usize, bool), i32>,
    rmat: [i32; 4],
    rmax: u32,
    /// 稀有度边框图标（frames.txt）：(原图标, 稀有度) → 带框的图标
    ficon: HashMap<(u16, usize), u16>,
    /// 自制道具图标（frames.txt 的 custom 行，make_frames.py 画的）：名字 → 图标（fusion_heart = 融锻之心）
    cicon: HashMap<String, u16>,
    /// 词条百科的原图标（affixes.txt 的 famicon 行，按稀有度）：键 → 四档。ART_LO..ART_HI 的是新词条饰品图标的虚编号，护符模板按它换图标
    famicon: HashMap<String, [u16; 4]>,
    /// 词条数值随机的占位效果行（valpool 行，gear/valfx.rs）
    valpool: Vec<i32>,
    /// 防具减伤倍率（armor_negation）：各稀有度的区间，没有就照原版
    pdef: [Option<(f64, f64)>; 4],
    /// 盾牌随机（shield_guard_random，用户 2026-11-01）：武器类型 → GUARD_F 六项在原版同类盾里的值（排好序）；
    /// 各稀有度在里面抽的分位区间。没有就照本体
    wguard: HashMap<u16, [Vec<f32>; 6]>,
    wguard_q: [Option<(f64, f64)>; 4],
    /// 法器的法术补正基数（catalyst_base，用户 2026-11-03）：各稀有度的区间（原版 100），没有就照原版
    cbase: [Option<(f64, f64)>; 4],
    /// 全能（五项都有补正）的补正倍率（womni 行，settings 的 weapon_scaling.omni；用户 2026-11-24「只用一个参数控制」）：
    /// 写参数行时乘（同法器倍率），超限上限、装备工坊的数也乘；没有这一行是 1（omni_mult）
    womni: Option<f64>,
    /// 能施法的本体（wcast 行）：编号 → (能用魔法, 能用祷告)
    wcast: HashMap<u32, (bool, bool)>,
    /// 超限上限（rr/overlimit.py，用户 2026-11-03）：武器 (稀有度, 补正项数) → (中位, 原上限, 超限上限)（ovlw 行）；
    /// 防具各稀有度减伤倍率的 (中位, 原上限, 超限上限)（ovlp 行）。没有就不能超限
    ovlw: HashMap<(usize, u32), (f32, f32, f32)>,
    ovlp: [Option<(f64, f64, f64)>; 4],
}

/// 盾牌随机的六项：五种属性的防御时减伤（%）、防御强度
const GUARD_F: [&str; 6] = ["physGuardCutRate", "magGuardCutRate", "fireGuardCutRate", "thunGuardCutRate", "darkGuardCutRate", "staminaGuardDef"];

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines().filter_map(|l| {
        let l = l.trim_start_matches('\u{feff}').trim_end_matches('\r');
        (!l.is_empty() && !l.starts_with('#')).then(|| l.split('\t').collect())
    })
}

fn parse_items(text: &str) -> Option<Items> {
    let mut d = Items::default();
    for f in lines(text) {
        match (f[0], f.len()) {
            ("container", 6..) => {
                let c = CAT_CODE.iter().position(|x| x.to_string() == f[1])?;
                let keep = if f.len() > 6 && !f[6].is_empty() {
                    f[6].split(',').map(|k| k.split_once(':').and_then(|(o, n)| Some((o.parse().ok()?, n.parse().ok()?)))).collect::<Option<Vec<_>>>()?
                } else {
                    Vec::new()
                };
                let second = match (f.get(7).and_then(|x| x.parse().ok()), f.get(8).and_then(|x| x.parse().ok())) {
                    (Some(first2), Some(split)) => Some((first2, split)),
                    _ => None,
                };
                d.cont[c] = Cont { first: f[2].parse().ok()?, step: f[3].parse().ok()?, count: f[4].parse().ok()?, size: f[5].parse().ok()?, keep, second };
            }
            ("text", 3..) => {
                let k = FIELDS.iter().position(|x| x.to_string() == f[1])?;
                d.cap[k] = f[2].parse().ok()?;
            }
            ("label", 3..) => {
                let k = ["zh", "en"].iter().position(|x| *x == f[1])?;
                d.label[k] = f[2].to_string();
            }
            ("goods", 4..) => {
                d.goods.insert(f[3].parse().ok()?, (f[1].parse().ok()?, f[2].parse().ok()?));
            }
            ("deliver", 4..) => (d.deliver_lot, d.deliver_rows, d.deliver_flag) = (f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?),
            ("identify", 2..) => d.identify_flag = f[1].parse().ok()?,
            // 看不懂当没有（别的定义照用）
            ("deathloss", 2..) => d.deathloss = f[1].parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map_or(0.0, |v| v.min(100.0)),
            // 看不懂当没有（照旧走发货表）
            ("deathback", 5..) => d.deathback = deathloss::Back::parse(&f),
            // 掉落池一行看不懂只丢这一行（别的定义照用）
            ("pool", 7..) => d.pools.extend(batch::Pool::parse(&f)),
            ("dirident", 9..) => {
                // 五档各一个价，逗号分隔；只写一个数就五档一样
                let five = |s: &str| -> Option<[u32; 5]> {
                    let v: Vec<u32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
                    match v.as_slice() {
                        [a] => Some([*a; 5]),
                        [a, b, c, e, g, ..] => Some([*a, *b, *c, *e, *g]),
                        _ => None,
                    }
                };
                let cost = five(f[7])?;
                // 防具 / 护符鉴定（用户 2026-11-09）：种类标记（第一个、几位）、护符每件的卢恩、流派名；旧的 items.txt 没有，就只有武器
                let num_at = |i: usize| f.get(i).and_then(|x| x.parse().ok()).unwrap_or(0u32);
                d.dirid = Some(DirId {
                    flag: f[1].parse().ok()?,
                    type0: (f[2].parse().ok()?, f[3].parse().ok()?),
                    rar0: (f[4].parse().ok()?, f[5].parse().ok()?),
                    none: f[6].parse().ok()?,
                    cost,
                    types: f[8].split(',').filter_map(|x| x.parse().ok()).collect(),
                    kind0: (num_at(9), num_at(10)),
                    cost_t: f.get(11).and_then(|x| five(x)).unwrap_or(cost),
                    archs: f.get(12).map_or_else(Vec::new, |x| x.split(',').filter(|a| !a.is_empty()).map(str::to_string).collect()),
                })
                .filter(|x| x.flag > 0 && !x.types.is_empty())
            }
            ("buyback", 5..) => {
                d.buyback = Some(Buyback { row0: f[1].parse().ok()?, n: f[2].parse().ok()?, rel0: f[3].parse().ok()?, stock0: f[4].parse().ok()? })
                    .filter(|b| b.n > 0 && b.rel0 > 0 && b.stock0 > 0)
            }
            ("wshop", 4..) => d.wshop.push((f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse::<u8>().ok()?.min(4))),
            ("reforge", 4..) => d.reforge = Some(Reforge { goods: f[1].parse().ok()?, mark: f[2].parse().ok()?, fx: f[3].parse().ok()? }),
            ("recycle", 4..) => {
                d.recycle = Some(Recycle {
                    goods: f[1].parse().ok()?,
                    flags: arr(f[2])?,
                    stones: arr(f[3])?,
                    // 第 4 个数是「回收全部弃置」的请求标记（用户 2026-11-26）；旧 items.txt 没有这一栏 → 0 = 没有这个功能
                    discard: f.get(4).and_then(|x| x.parse().ok()).unwrap_or(0),
                })
            }
            ("overlimit", 4..) => {
                let steps: u32 = f[3].parse().ok()?;
                let asteps = f.get(4).and_then(|x| x.parse().ok()).filter(|&x: &u32| x > 0).unwrap_or(steps);
                let wspread = f.get(5).and_then(|x| x.parse::<f32>().ok()).filter(|x| x.is_finite()).map_or(0.0, |x| x.clamp(0.0, 0.9));
                d.overlimit = Some(Overlimit { gw: f[1].parse().ok()?, ga: f[2].parse().ok()?, steps, asteps, wspread }).filter(|o| o.steps > 0)
            }
            ("inherit", 4..) => {
                let num = |i: usize| f.get(i).and_then(|x| x.parse().ok()).unwrap_or(0);
                d.inherit = Some(Inherit { open: f[1].parse().ok()?, ack: f[2].parse().ok()?, noui: f[3].parse().ok()?, goods: num(4), pages: [num(5), num(6)] })
            }
            ("startids", 5..) => d.start_ids = [(f[1].parse().ok()?, f[2].parse().ok()?), (f[3].parse().ok()?, f[4].parse().ok()?)],
            ("lotlayout", 7..) => {
                d.lot_size = f[1].parse().ok()?;
                for k in 0..5 {
                    d.lot_off[k] = f[2 + k].parse().ok()?;
                }
            }
            ("start", 7..) => d.start.push(StartSpec {
                cat: f[1].parse().ok().filter(|&c: &usize| c < 2)?,
                id: f[2].parse().ok()?,
                tier: f[3].parse().ok()?,
                base: f[4].parse().ok()?,
                seed: f[5].parse().ok()?,
                maxreq: arr(f[6])?,
                top: f.get(7).map_or(Some(100.0), |x| x.parse::<f64>().ok())? / 100.0,
                min_status: f.get(8).map_or(Some(0), |x| x.parse().ok())?,
                keep: f.get(9).is_some_and(|x| *x == "1"),
                main: f.get(10).map_or(Some(-1), |x| x.parse().ok())?,
                pick: f.get(11).is_some_and(|x| *x == "1"),
                full: f.get(12).is_some_and(|x| *x == "1"),
            }),
            ("wish", 8..) => {
                let w = d.wish.get_or_insert_with(Wish::default);
                (w.flag1, w.flag10, w.free0, w.free_n) = (f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?, f[4].parse().ok()?);
                for k in 0..NCAT {
                    w.share[k] = f[5 + k].parse().ok()?;
                }
                w.poor = f.get(5 + NCAT).map_or(Some(0), |x| x.parse().ok())?;
            }
            ("wband", 8..) => {
                let w = d.wish.get_or_insert_with(Wish::default);
                let k: usize = f[1].parse().ok()?;
                if k != w.bands.len() {
                    return None;
                }
                w.bands.push(WishBand { cost: f[2].parse().ok()?, dlc: f[3] == "1", w: arr(&f[4..8].join(","))? });
            }
            ("wmap", 6..) => {
                let w = d.wish.get_or_insert_with(Wish::default);
                w.maps.insert((f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?, f[4].parse().ok()?), f[5].parse().ok()?);
            }
            _ => {}
        }
    }
    if d.wish.as_ref().is_some_and(|w| w.flag1 == 0 || w.bands.is_empty()) {
        d.wish = None;
    }
    let ok = d.cont.iter().all(|c| c.count > 0 && c.step > 0 && c.size > 0)
        && d.cap.iter().all(|&c| c > 0)
        && d.label.iter().all(|l| !l.is_empty())
        && !d.goods.is_empty()
        && d.deliver_lot > 0
        && d.deliver_rows > 0
        && d.deliver_flag > 0
        && d.identify_flag > 0
        && d.lot_size > 0;
    ok.then_some(d)
}

fn parse_lib(text: &str) -> Result<Lib, String> {
    let mut d = Lib { wstat_sp: vec![HashMap::new(); STATUS_N], wstat_msg: [-1; STATUS_N], ..Default::default() };
    let num = |s: &str| s.parse::<i64>().map_err(|_| format!("不是数字：{s}"));
    let dist = |f: &[&str]| Dist::parse(f).ok_or_else(|| format!("分布格式不对：{}", f.join(" ")));
    let r_of = |s: &str| -> Result<usize, String> { num(s).map(|v| v as usize).and_then(|v| if v < 4 { Ok(v) } else { Err(format!("稀有度 {v}")) }) };
    let numf = |s: &str| s.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("不是数字：{s}"));
    // 词条数值随机：词条序号 → [(数的位置, 显示精度, 是不是主数, 各档的数)]
    let mut val_nums: HashMap<usize, Vec<(usize, f32, bool, [Option<f32>; 4])>> = HashMap::new();
    for f in lines(text) {
        let n = f.len();
        match f[0] {
            "rar" if n >= 4 => d.rar[r_of(f[1])?] = [unesc(f[2]), unesc(f[3])],
            "sell" if n >= 6 => {
                let c = CAT_CODE.iter().position(|x| x.to_string() == f[1]).ok_or("sell 的类不对")?;
                for k in 0..4 {
                    d.sell[c][k] = num(f[2 + k])? as i32;
                }
            }
            "stat" if n >= 3 => d.stat.push([unesc(f[1]), unesc(f[2])]),
            "field" if n >= 6 => {
                let c = f[1].chars().next().ok_or("field 的类不对")?;
                let fmt = f[4].bytes().next().ok_or("field 的格式不对")?;
                d.fields.insert((c, f[2].to_string()), Field { off: num(f[3])? as usize, fmt, bit: num(f[5])? as i32 });
            }
            "wbase" if n >= 7 => d.wbase.push(WBase {
                typ: num(f[1])? as u16,
                id: num(f[2])? as u32,
                var: num(f[3])? as i32,
                elem: (num(f[4])? as usize).min(4),
                name: [unesc(f[5]), unesc(f[6])],
            }),
            "pbase" if n >= 5 => d.pbase.push(PBase { slot: num(f[1])? as u8, id: num(f[2])? as u32, name: [unesc(f[3]), unesc(f[4])] }),
            "wtype" if n >= 2 => d.wtype[r_of(f[1])?] = dist(&f[2..])?,
            "wscal_n" if n >= 2 => d.wscal_n[r_of(f[1])?] = dist(&f[2..])?,
            "wscal_v" if n >= 3 => _ = d.wscal_v.insert((r_of(f[1])?, num(f[2])? as u32), dist(&f[3..])?),
            "wreq" if n >= 3 => _ = d.wreq.insert((r_of(f[1])?, num(f[2])? as usize), dist(&f[3..])?),
            "wreq_rule" if n >= 6 => {
                d.wreq_sorted = f[1] == "1";
                for r in 0..4 {
                    d.wreq_minlvl[r] = num(f[2 + r])?.max(0) as u32;
                }
            }
            "wfull_nostatus" if n >= 2 => d.full_no_status = f[1] == "1",
            "bias" if n >= 2 => d.bias_n = num(f[1])?.clamp(1, 16) as u32,
            "wcurve" if n >= 3 => _ = d.wcurve.insert((r_of(f[1])?, num(f[2])? as usize), dist(&f[3..])?),
            "wcurvet" if n >= 4 => {
                let ks = f[2].split(',').filter(|s| !s.is_empty()).map(|s| num(s).map(|k| k as usize)).collect::<Result<Vec<_>, _>>()?;
                if ks.is_empty() || ks.iter().any(|&k| k >= CURVE_N) {
                    return Err(format!("wcurvet 的伤害种类不对：{}", f[2]));
                }
                d.wcurvet.entry(num(f[1])?.clamp(0, u16::MAX as i64) as u16).or_default().push((ks, dist(&f[3..])?));
            }
            "wstat_n" if n >= 2 => d.wstat_n[r_of(f[1])?] = dist(&f[2..])?,
            "wspec_n" if n >= 2 => d.wspec_n[r_of(f[1])?] = dist(&f[2..])?,
            "wassault" if n >= 4 => d.wassault[r_of(f[1])?] = (num(f[2])?.clamp(0, 100) as u32, num(f[3])?.clamp(0, 255) as u8),
            "wcrit" if n >= 3 => {
                let m = f[2].parse::<f64>().map_err(|_| format!("不是数字：{}", f[2]))?;
                if !(m > 0.0 && m < 10.0) {
                    return Err(format!("致命一击倍率不对：{m}"));
                }
                d.wcrit[r_of(f[1])?] = m;
            }
            "wheavy" if n >= 3 => {
                let m = f[1].parse::<f64>().map_err(|_| format!("不是数字：{}", f[1]))?;
                if !(m > 0.0 && m < 10.0) {
                    return Err(format!("重武器的基础攻击力倍率不对：{m}"));
                }
                for t in f[2].split(',').filter(|s| !s.is_empty()) {
                    d.wheavy.insert(num(t)?.clamp(0, u16::MAX as i64) as u16, m);
                }
            }
            "rmax" if n >= 2 => d.rmax = num(f[1])?.clamp(0, 25) as u32,
            "pdef" if n >= 4 => {
                let fl = |s: &str| s.parse::<f64>().map_err(|_| format!("不是数字：{s}"));
                let (lo, hi) = (fl(f[2])?, fl(f[3])?);
                if !(lo > 0.0 && hi >= lo && hi < 10.0) {
                    return Err(format!("防具减伤倍率区间不对：{lo} ~ {hi}"));
                }
                d.pdef[r_of(f[1])?] = Some((lo, hi));
            }
            "womni" if n >= 2 => {
                let m = f[1].parse::<f64>().map_err(|_| format!("不是数字：{}", f[1]))?;
                if !(m > 0.0 && m <= 1.0) {
                    return Err(format!("全能补正倍率不对：{m}"));
                }
                d.womni = Some(m);
            }
            "cbase" if n >= 4 => {
                let fl = |s: &str| s.parse::<f64>().map_err(|_| format!("不是数字：{s}"));
                let (lo, hi) = (fl(f[2])?, fl(f[3])?);
                if !(lo > 0.0 && hi >= lo && hi < 1000.0) {
                    return Err(format!("法器倍率区间不对：{lo} ~ {hi}"));
                }
                d.cbase[r_of(f[1])?] = Some((lo, hi));
            }
            "wcast" if n >= 4 => _ = d.wcast.insert(num(f[1])? as u32, (f[2] == "1", f[3] == "1")),
            "ovlw" if n >= 6 => {
                let fl = |s: &str| s.parse::<f32>().map_err(|_| format!("不是数字：{s}"));
                let (m, hi, cap) = (fl(f[3])?, fl(f[4])?, fl(f[5])?);
                if !(m > 0.0 && hi >= m && cap >= hi) {
                    return Err(format!("武器超限上限不对：{m} {hi} {cap}"));
                }
                d.ovlw.insert((r_of(f[1])?, num(f[2])? as u32), (m, hi, cap));
            }
            "ovlp" if n >= 5 => {
                let fl = |s: &str| s.parse::<f64>().map_err(|_| format!("不是数字：{s}"));
                let (m, hi, cap) = (fl(f[2])?, fl(f[3])?, fl(f[4])?);
                if !(m > 0.0 && hi >= m && cap >= hi && cap < 10.0) {
                    return Err(format!("防具超限上限不对：{m} {hi} {cap}"));
                }
                d.ovlp[r_of(f[1])?] = Some((m, hi, cap));
            }
            "wguard" if n >= 4 => {
                let k = num(f[2])? as usize;
                if k >= GUARD_F.len() {
                    return Err(format!("盾牌字段序号不对：{k}"));
                }
                let mut v = f[3].split(',').filter(|s| !s.is_empty()).map(|s| s.parse::<f32>().map_err(|_| format!("不是数字：{s}"))).collect::<Result<Vec<_>, _>>()?;
                v.sort_by(|a, b| a.total_cmp(b));
                d.wguard.entry(num(f[1])?.clamp(0, u16::MAX as i64) as u16).or_default()[k] = v;
            }
            "wguard_q" if n >= 4 => {
                let fl = |s: &str| s.parse::<f64>().map_err(|_| format!("不是数字：{s}"));
                let (lo, hi) = (fl(f[2])?, fl(f[3])?);
                if !(0.0..=1.0).contains(&lo) || !(lo..=1.0).contains(&hi) {
                    return Err(format!("盾牌分位区间不对：{lo} ~ {hi}"));
                }
                d.wguard_q[r_of(f[1])?] = Some((lo, hi));
            }
            "rmat" if n >= 5 => {
                for k in 0..4 {
                    d.rmat[k] = num(f[1 + k])? as i32;
                }
            }
            "reinf" if n >= 5 => _ = d.reinf.insert((num(f[1])? as u16, r_of(f[2])?, f[3] == "1"), num(f[4])? as i32),
            "pslot_p" if n >= 5 => {
                let t = num(f[1])? as usize;
                if t > 4 {
                    return Err(format!("档 {t}"));
                }
                d.pslot_p[t] = Some([num(f[2])?.clamp(0, 100) as u32, num(f[3])?.clamp(0, 100) as u32, num(f[4])?.clamp(0, 100) as u32]);
            }
            "wspec_p" if n >= 3 => {
                let t = num(f[1])? as usize;
                if t > 4 {
                    return Err(format!("档 {t}"));
                }
                d.wspec_p[t] = Some(num(f[2])?.clamp(0, 100) as u32);
            }
            "wspec" if n >= 9 => d.wspec.push(WSpec {
                key: f[1].to_string(),
                sp: num(f[2])? as i32,
                w: num(f[3])? as u32,
                types: f[4].split(',').filter_map(|x| x.trim().parse().ok()).collect(),
                name: [unesc(f[5]), unesc(f[6])],
                text: [unesc(f[7]), unesc(f[8])],
                rar: f.get(9).map_or(Ok(15), |x| num(x))? as u8,
            }),
            "wstat_pow" if n >= 3 => _ = d.wstat_pow.insert((num(f[1])? as usize, r_of(f[2])?), dist(&f[3..])?),
            "wstat_sp" if n >= 2 => {
                let k = num(f[1])? as usize;
                if k >= STATUS_N {
                    return Err(format!("异常种类 {k}"));
                }
                for p in &f[2..] {
                    let (a, b) = p.split_once(':').ok_or("wstat_sp 格式不对")?;
                    d.wstat_sp[k].insert(num(a)? as u32, num(b)? as i32);
                }
            }
            "wstat_msg" if n >= 3 => {
                let k = num(f[1])? as usize;
                if k >= STATUS_N {
                    return Err(format!("异常种类 {k}"));
                }
                d.wstat_msg[k] = num(f[2])? as i32;
            }
            "element_table" if n >= 2 => d.element_table = num(f[1])? as i32,
            // welem <拆分元素占比下限 %> <上限 %> <拆分补偿系数> <纯元素倍率> <不抽的武器类型,…>
            "welem" if n >= 5 => {
                let fl = |s: &str| s.parse::<f64>().map_err(|_| format!("不是数字：{s}"));
                let (lo, hi) = (num(f[1])?.clamp(1, 99) as u8, num(f[2])?.clamp(1, 99) as u8);
                d.welem = WElem {
                    on: true,
                    lo: lo.min(hi),
                    hi: hi.max(lo),
                    coef: fl(f[3])?.clamp(0.0, 2.0),
                    pure: fl(f[4])?.clamp(0.5, 3.0),
                    skip: f.get(5).map_or(Vec::new(), |s| s.split(',').filter_map(|x| x.parse().ok()).collect()),
                };
            }
            "glow" if n >= 3 => d.glow[(num(f[1])? as usize).min(4)] = num(f[2])? as i32,
            "wres_types" if n >= 3 => res_mut(&mut d.res, f[1]).types = f[2].split(',').filter_map(|x| x.parse().ok()).collect(),
            "wres_n" if n >= 3 => res_mut(&mut d.res, f[1]).n[r_of(f[2])?] = dist(&f[3..])?,
            "wres" if n >= 6 => {
                let (sp, w) = (num(f[2])? as i32, num(f[3])? as u32);
                res_mut(&mut d.res, f[1]).pool.push((sp, w, [unesc(f[4]), unesc(f[5])]));
            }
            "skill" if n >= 2 => _ = d.skill.insert(num(f[1])? as u16, dist(&f[2..])?),
            "skill_def" if n >= 2 => _ = d.skill_def.insert(num(f[1])? as u16, dist(&f[2..])?),
            "skillinfo" if n >= 6 => _ = d.skillinfo.insert(
                num(f[1])? as i32,
                SkillInfo { gem: f[2] == "1", native: num(f[3])? as i32, name: [unesc(f[4]), unesc(f[5])] },
            ),
            "skillvar" if n >= 4 => _ = d.skillvar.insert(num(f[1])? as i32, (num(f[2])? as i32, num(f[3])? as i32)),
            "pslot" if n >= 2 => d.pslot[r_of(f[1])?] = dist(&f[2..])?,
            "pcount" if n >= 2 => {
                let t = num(f[1])? as usize;
                if t > 4 {
                    return Err(format!("档 {t}"));
                }
                d.pcount[t] = dist(&f[2..])?;
            }
            "fam" if n >= 7 => {
                let k = num(f[1])? as usize;
                if k != d.fams.len() {
                    return Err(format!("词条序号不连续：{k}"));
                }
                d.fam_by_key.insert(f[6].to_string(), k);
                d.fams.push(Fam {
                    key: f[6].to_string(),
                    armor_w: [num(f[2])? as u32; 4],
                    tal_w: [num(f[3])? as u32; 4],
                    suffix: [unesc(f[4]), unesc(f[5])],
                    slots: f.get(7).map_or(Ok(15), |x| num(x))? as u8,
                    mech: f.get(8).is_some_and(|x| *x == "1"),
                    group: f.get(9).filter(|x| **x != "-").map_or(String::new(), |x| x.to_string()),
                    r: [None, None, None, None],
                });
                let a = f.get(10).map_or_else(|| "其他".to_string(), |x| unesc(x));
                let id = d.arch_names.iter().position(|x| *x == a).unwrap_or_else(|| {
                    d.arch_names.push(a.clone());
                    d.arch_names.len() - 1
                });
                d.fam_arch.push(id as u8);
            }
            "famr" if n >= 7 => {
                let k = num(f[1])? as usize;
                let r = r_of(f[2])?;
                let fam = d.fams.get_mut(k).ok_or_else(|| format!("famr 的词条 {k} 不存在"))?;
                fam.r[r] = Some(FamR { sp: num(f[3])? as i32, tal: num(f[4])? as i32, text: [unesc(f[5]), unesc(f[6])], val: None });
            }
            // 词条数值随机（rr/affixes.py val_lines）：famnum <序号> <数的位置> <精度> <1 = 主数> <各档的数>；其余几种行 <序号> <r> …（跟在这一档的 famr 后面）
            "famnum" if n >= 6 => {
                let k = num(f[1])? as usize;
                let c: Vec<Option<f32>> = f[5].split(',').map(|x| x.parse().ok()).collect();
                if c.len() != 4 {
                    return Err(format!("famnum 的各档数不是 4 个：{}", f.join(" ")));
                }
                val_nums.entry(k).or_default().push((num(f[2])? as usize, numf(f[3])?, f[4] == "1", [c[0], c[1], c[2], c[3]]));
            }
            "famval" | "famtpl" | "famspan" | "famrow" | "famref" | "famfld" | "famrnum" | "famslot" | "famrun" if n >= 4 => {
                let k = num(f[1])? as usize;
                let r = r_of(f[2])?;
                let fr = d.fams.get_mut(k).and_then(|x| x.r[r].as_mut()).ok_or_else(|| format!("{} 的词条 {k} 稀有度 {r} 没有 famr", f[0]))?;
                let v = fr.val.get_or_insert_with(Default::default);
                match (f[0], n) {
                    ("famval", 7..) => (v.lo, v.hi, v.mu, v.sigma) = (numf(f[3])?, numf(f[4])?, numf(f[5])?, numf(f[6])?),
                    ("famtpl", 5..) => v.tpl = [unesc(f[3]), unesc(f[4])],
                    ("famspan", 5..) => v.span = [unesc(f[3]), unesc(f[4])],
                    ("famrow", 4..) => v.rows = f[3].split(',').map(|x| x.parse().map_err(|_| format!("famrow：{}", f.join(" ")))).collect::<Result<_, _>>()?,
                    ("famref", 6..) => v.refs.push((num(f[3])? as usize, num(f[4])? as usize, num(f[5])? as usize)),
                    ("famrnum", 8..) => v.rnum.push((num(f[3])? as usize, f[4].to_string(), numf(f[5])?, numf(f[6])?, num(f[7])? as usize)),
                    ("famslot", 5..) => {
                        let ws = f[4]
                            .split(';')
                            .filter(|x| !x.is_empty())
                            .map(|w| {
                                let c: Vec<&str> = w.split(':').collect();
                                if c.len() != 5 {
                                    return Err(format!("famslot 的字段：{w}"));
                                }
                                Ok((num(c[0])? as usize, c[1].bytes().next().unwrap_or(b'?'), numf(c[2])?, numf(c[3])?, num(c[4])? as usize))
                            })
                            .collect::<Result<Vec<_>, String>>()?;
                        v.slots.push((num(f[3])? as i32, ws));
                    }
                    ("famrun", 8..) => v.runs.push((f[3].to_string(), numf(f[5])?, numf(f[6])?, num(f[7])? as usize)),
                    ("famfld", 9..) => v.fields.push((num(f[3])? as usize, num(f[4])? as usize, f[5].bytes().next().unwrap_or(b'?'), numf(f[6])?, numf(f[7])?, num(f[8])? as usize)),
                    _ => return Err(format!("{} 行的列数不对：{}", f[0], f.join(" "))),
                }
            }
            // valpool <编号,…>：词条数值随机的占位效果行（gear/valfx.rs）
            "valpool" if n >= 2 => d.valpool = f[1].split(',').filter_map(|x| x.parse().ok()).collect(),
            // famicon <键> <原图标 普通,稀有,史诗,传说>（词条百科用的；这里只认新词条饰品图标的虚编号）
            "famicon" if n >= 3 => {
                let v: Vec<u16> = f[2].split(',').filter_map(|x| x.parse().ok()).collect();
                if let Ok(v) = <[u16; 4]>::try_from(v) {
                    d.famicon.insert(f[1].to_string(), v);
                }
            }
            // famw <序号> <防具权重 普通,稀有,史诗,传说> <护符权重 …>
            "famw" if n >= 4 => {
                let k = num(f[1])? as usize;
                let fam = d.fams.get_mut(k).ok_or_else(|| format!("famw 的词条 {k} 不存在"))?;
                let four = |s: &str| -> Result<[u32; 4], String> {
                    let v = s.split(',').map(|x| num(x).map(|w| w as u32)).collect::<Result<Vec<_>, _>>()?;
                    v.try_into().map_err(|_| format!("famw 的词条 {k} 不是四档权重"))
                };
                fam.armor_w = four(f[2])?;
                fam.tal_w = four(f[3])?;
            }
            "famtop" if n >= 2 => {
                let k = num(f[1])? as usize;
                let key = d.fams.get(k).ok_or_else(|| format!("famtop 的词条 {k} 不存在"))?.key.clone();
                d.top_only.insert(key);
            }
            _ => {}
        }
    }
    if d.wbase.is_empty() || d.pbase.is_empty() || d.fams.is_empty() || d.stat.len() != 5 || d.rar[0][0].is_empty() {
        return Err("词条库不完整（本体、词条、属性名或稀有度缺了）".into());
    }
    for (c, names) in [('w', &["correctStrength", "swordArtsParamId", "behaviorVariationId", "rarity", "sellValue", "disableGemAttr"][..]),
        ('p', &["residentSpEffectId", "rarity", "sellValue"][..]), ('a', &["rarity", "sellValue"][..])]
    {
        for nm in names {
            if !d.fields.contains_key(&(c, nm.to_string())) {
                return Err(format!("缺字段偏移 {c} {nm}"));
            }
        }
    }
    // 词条数值随机：famnum 的主数 / 插值的数补进各档的定义；不完整的这一档照固定数值
    for (k, f) in d.fams.iter_mut().enumerate() {
        let nums = val_nums.get(&k);
        for (r, fr) in f.r.iter_mut().enumerate() {
            let Some(fr) = fr else { continue };
            let Some(v) = fr.val.as_mut() else { continue };
            let main = nums.and_then(|x| x.iter().find(|n| n.2));
            let ok = main.is_some_and(|m| {
                v.main = m.0;
                v.prec = m.1;
                v.c = m.3[r].unwrap_or(f32::NAN);
                v.others = nums
                    .into_iter()
                    .flatten()
                    .filter(|n| !n.2)
                    .map(|n| {
                        let mut pts: Vec<(f32, f32)> = (0..4).filter_map(|t| Some((m.3[t]?, n.3[t]?))).collect();
                        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                        (n.0, n.1, pts)
                    })
                    .collect();
                v.lo < v.hi && v.lo <= v.c && v.c <= v.hi && v.sigma > 0.0 && v.prec > 0.0 && !v.tpl[0].is_empty()
                    && (!v.rows.is_empty() || !v.rnum.is_empty() || !v.slots.is_empty() || !v.runs.is_empty())
                    && v.fields.iter().all(|x| x.0 < v.rows.len()) && v.refs.iter().all(|x| x.0 < v.rows.len() && x.2 < v.rows.len())
            });
            if !ok {
                log(&format!("词条数值随机：{} 稀有度 {r} 的取值定义不完整，照固定数值", f.key));
                fr.val = None;
            }
        }
    }
    Ok(d)
}

fn res_mut<'a>(v: &'a mut Vec<(String, Res)>, name: &str) -> &'a mut Res {
    if let Some(k) = v.iter().position(|x| x.0 == name) {
        return &mut v[k].1;
    }
    v.push((name.to_string(), Res::default()));
    &mut v.last_mut().unwrap().1
}

struct Defs {
    items: Items,
    lib: Lib,
}

static DEFS: OnceLock<Option<Defs>> = OnceLock::new();

fn defs() -> Option<&'static Defs> {
    DEFS.get_or_init(|| {
        let it = fs::read_to_string(dir().join("items.txt")).ok()?;
        let Some(items) = parse_items(&it) else {
            log("新装备系统：items.txt 格式不对，不工作");
            return None;
        };
        crate::msghook::set_labels(&items.label); // 取文字钩子按占位里的说明认语言
        let lt = fs::read_to_string(dir().join("affixes.txt")).ok();
        match lt.map(|t| parse_lib(&t)) {
            Some(Ok(mut lib)) => {
                let ft = fs::read_to_string(dir().join("frames.txt")).unwrap_or_default();
                lib.ficon = parse_frames(&ft);
                lib.cicon = parse_custom_icons(&ft);
                if !lib.ficon.is_empty() {
                    log(&format!("新装备系统：稀有度边框图标 {} 个、自制道具图标 {} 个（frames.txt）", lib.ficon.len(), lib.cicon.len()));
                }
                Some(Defs { items, lib })
            }
            Some(Err(e)) => {
                log(&format!("新装备系统：affixes.txt 读不了（{e}），不工作"));
                None
            }
            None => {
                log("新装备系统：没有 affixes.txt，不工作");
                None
            }
        }
    })
    .as_ref()
}

pub fn describe() -> String {
    match defs() {
        Some(d) => format!(
            "新装备系统（容器 武器 {} / 防具 {} / 护符 {}，本体 武器 {} / 防具 {}，词条 {} 种，开局装备 {} 件，回购 {} 格）；{}",
            d.items.cont[0].count,
            d.items.cont[1].count,
            d.items.cont[2].count,
            d.lib.wbase.len(),
            d.lib.pbase.len(),
            d.lib.fams.len(),
            d.items.start.len(),
            d.items.buyback.map_or(0, |b| b.n),
            ashes::describe()
        ),
        None => "新装备系统（没有 items.txt / affixes.txt，不管）".into(),
    }
}

// ------------------------------------------------------------------ 随机数（splitmix64）
struct Rng(u64);

static SEED_N: AtomicU64 = AtomicU64::new(0);

impl Rng {
    fn new() -> Rng {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        Rng(t ^ SEED_N.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed).rotate_left(17))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 从 0..n 里不重复地挑 k 个
fn choose(rng: &mut Rng, n: usize, k: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let k = k.min(n);
    for i in 0..k {
        let j = i + rng.below((n - i) as u64) as usize;
        v.swap(i, j);
    }
    v.truncate(k);
    v
}

/// 按权重不重复地挑 k 个（返回下标）
fn choose_weighted(rng: &mut Rng, w: &[u32], k: usize) -> Vec<usize> {
    let mut left: Vec<(usize, u32)> = w.iter().copied().enumerate().filter(|x| x.1 > 0).collect();
    let mut out = Vec::new();
    while out.len() < k && !left.is_empty() {
        let tot: u64 = left.iter().map(|x| x.1 as u64).sum();
        let mut x = rng.below(tot);
        let mut at = left.len() - 1;
        for (j, &(_, wt)) in left.iter().enumerate() {
            if x < wt as u64 {
                at = j;
                break;
            }
            x -= wt as u64;
        }
        out.push(left.remove(at).0);
    }
    out
}

// ------------------------------------------------------------------ 配方
#[derive(Debug, Clone, PartialEq)]
enum Recipe {
    /// 武器：本体、行为变体、战技（-1 = 照本体）、补正、能力需求、补正曲线、命中异常、常驻效果、传说光效、特效、强攻（基础攻击力 +%，0 = 没有）
    /// ovl：超限过（用过超限锻造石，补正可能超过原区间；不能再重铸）
    /// elem：属性（0 = 照本体（旧记录 / 追忆成品 / 不抽的类型），1 纯物理，2~5 物理＋魔力 / 火 / 雷 / 圣，6~9 纯魔力 / 火 / 雷 / 圣）；share：拆分时元素占比 %
    W { base: u32, var: i32, skill: i32, cor: [f32; 5], req: [u8; 5], curve: [u8; CURVE_N], st: [i32; 3], res: [i32; 2], glow: i32, spec: i32, atk: u8, ovl: bool, elem: u8, share: u8 },
    /// 防具：本体、（效果, 词条键）× 1~3、减伤倍率（超限过才存；没有 = 按记录算 armor_mult）、
    /// pos：词条数值随机的位置（千分之几，和 sps 一一对应，少的 / 固定数值的词条是 0；记录里写成「效果:键:位置」）
    P { base: u32, sps: Vec<(i32, String)>, mult: Option<f64>, pos: Vec<i16> },
    /// 护符：模板、效果、词条键、词条数值随机的位置（千分之几；记录第 7 栏，没有是 0）
    A { tal: u32, sp: i32, key: String, pos: i16 },
}

#[derive(Debug, Clone, PartialEq)]
struct Rec {
    cat: usize,
    idx: u32,
    /// 档 0~4（4 = 传说（幽影之地））
    tier: u8,
    recipe: Recipe,
}

impl Rec {
    fn rar(&self) -> usize {
        (self.tier as usize).min(3)
    }
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
}

fn rec_line(r: &Rec) -> String {
    let head = format!("{}\t{}\t{}", CAT_CODE[r.cat], r.idx, r.tier);
    match &r.recipe {
        Recipe::W { base, var, skill, cor, req, curve, st, res, glow, spec, atk, ovl, elem, share } => {
            // 超限过才写（旧记录没有这一栏）；有属性随机的再写 超限 0/1、属性、元素占比
            let tail = if *elem > 0 { format!("\t{}\t{elem}\t{share}", *ovl as u8) } else if *ovl { "\t1".to_string() } else { String::new() };
            format!("{head}\t{base}\t{var}\t{skill}\t{}\t{}\t{}\t{}\t{}\t{glow}\t{spec}\t{atk}{tail}", join(cor), join(req), join(curve), join(st), join(res))
        }
        Recipe::P { base, sps, mult, pos } => {
            let tail = mult.map_or(String::new(), |m| format!("\t{m}"));
            let one = |i: usize, s: &i32, k: &String| match pos.get(i).copied().unwrap_or(0) {
                0 => format!("{s}:{k}"),
                p => format!("{s}:{k}:{p}"),
            };
            format!("{head}\t{base}\t{}{tail}", sps.iter().enumerate().map(|(i, (s, k))| one(i, s, k)).collect::<Vec<_>>().join(","))
        }
        Recipe::A { tal, sp, key, pos } => {
            let tail = if *pos != 0 { format!("\t{pos}") } else { String::new() };
            format!("{head}\t{tal}\t{sp}\t{key}{tail}")
        }
    }
}

fn arr<T: std::str::FromStr + Copy + Default, const N: usize>(s: &str) -> Option<[T; N]> {
    let v: Vec<T> = s.split(',').map(|x| x.parse().ok()).collect::<Option<Vec<T>>>()?;
    (v.len() == N).then(|| {
        let mut a = [T::default(); N];
        a.copy_from_slice(&v);
        a
    })
}

fn rec_parse(line: &str) -> Option<Rec> {
    let f: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
    if f.len() < 4 {
        return None;
    }
    let cat = CAT_CODE.iter().position(|c| c.to_string() == f[0])?;
    let (idx, tier): (u32, u8) = (f[1].parse().ok()?, f[2].parse().ok()?);
    let recipe = match cat {
        0 if f.len() >= 12 => Recipe::W {
            base: f[3].parse().ok()?,
            var: f[4].parse().ok()?,
            skill: f[5].parse().ok()?,
            cor: arr(f[6])?,
            req: arr(f[7])?,
            curve: arr(f[8])?,
            st: arr(f[9])?,
            res: arr(f[10])?,
            glow: f[11].parse().ok()?,
            spec: f.get(12).and_then(|x| x.parse().ok()).unwrap_or(-1),
            atk: f.get(13).and_then(|x| x.parse().ok()).unwrap_or(0),
            ovl: f.get(14).is_some_and(|x| *x == "1"),
            elem: f.get(15).and_then(|x| x.parse().ok()).filter(|&e: &u8| e <= 9).unwrap_or(0),
            share: f.get(16).and_then(|x| x.parse().ok()).unwrap_or(0),
        },
        1 if f.len() >= 5 => {
            // 「效果:键」或「效果:键:位置」（词条数值随机）
            let v: Vec<(i32, String, i16)> = f[4]
                .split(',')
                .filter(|x| !x.is_empty())
                .map(|x| {
                    let mut it = x.splitn(3, ':');
                    let s = it.next()?.parse().ok()?;
                    let k = it.next()?.to_string();
                    let p = it.next().map_or(Some(0), |p| p.parse::<i16>().ok())?;
                    Some((s, k, p.clamp(-1000, 1000)))
                })
                .collect::<Option<Vec<_>>>()?;
            let pos: Vec<i16> = if v.iter().any(|x| x.2 != 0) { v.iter().map(|x| x.2).collect() } else { Vec::new() };
            Recipe::P {
                base: f[3].parse().ok()?,
                sps: v.into_iter().map(|x| (x.0, x.1)).collect(),
                mult: f.get(5).and_then(|x| x.parse::<f64>().ok()).filter(|m| m.is_finite() && *m > 0.0),
                pos,
            }
        }
        2 if f.len() >= 6 => Recipe::A {
            tal: f[3].parse().ok()?,
            sp: f[4].parse().ok()?,
            key: f[5].to_string(),
            pos: f.get(6).and_then(|x| x.parse::<i16>().ok()).unwrap_or(0).clamp(-1000, 1000),
        },
        _ => return None,
    };
    Some(Rec { cat, idx, tier, recipe })
}

// ------------------------------------------------------------------ 生成
fn gen_weapon(lib: &Lib, tier: usize, rng: &mut Rng) -> Option<Recipe> {
    gen_weapon_on(lib, tier, None, rng)
}

/// fixed：开局武器的约束（见 WFix）
fn gen_weapon_on(lib: &Lib, tier: usize, fixed: Option<WFix>, rng: &mut Rng) -> Option<Recipe> {
    gen_weapon_inner(lib, tier, fixed, None, rng)
}

/// 指定武器类型（暗黑商店的武器商店）：本体从这类型里等概率挑，别的照随机生成
fn gen_weapon_typed(lib: &Lib, tier: usize, typ: u16, rng: &mut Rng) -> Option<Recipe> {
    gen_weapon_inner(lib, tier, None, Some(typ), rng)
}

/// want：指定的武器类型（None = 按这一档的类型分布抽；不指定时随机数的顺序和以前一样，按种子生成的开局装备不变）
/// 抽属性：(elem, share, 光效)。没开 / 不抽的类型照本体（elem 0、光效不变）
fn draw_elem(lib: &Lib, typ: u16, r: usize, base_elem: usize, glow: i32, rng: &mut Rng) -> (u8, u8, i32) {
    let w = &lib.welem;
    if !w.on || w.skip.contains(&typ) {
        return (0, 0, glow);
    }
    let o = rng.below(9) as u8;
    let (elem, share) = match o {
        0 => (1, 0),
        1..=4 => (1 + o, w.lo + rng.below((w.hi - w.lo) as u64 + 1) as u8),
        _ => (1 + o, 100),
    };
    (elem, share, elem_glow(lib, r, elem, base_elem))
}

fn gen_weapon_inner(lib: &Lib, tier: usize, fixed: Option<WFix>, want: Option<u16>, rng: &mut Rng) -> Option<Recipe> {
    let r = tier.min(3);
    let base = match fixed {
        Some(f) => lib.wbase.iter().find(|b| b.id == f.base)?,
        None => {
            let typ = match want {
                Some(t) => t,
                None => lib.wtype[r].pick(rng)? as u16,
            };
            let bases: Vec<&WBase> = lib.wbase.iter().filter(|b| b.typ == typ).collect();
            *bases.get(rng.below(bases.len() as u64) as usize)?
        }
    };
    let typ = base.typ;
    // 补正：几项（1 / 2 / 3 / 5）、哪几项（五项等概率）、值、能力需求；开局武器不抽全能（五项），追忆的成品传说、商人的小极品传说（full）可以。
    // 先按原版比例定几项，再在这个项数的分布里取前 top：全能和别的项数各自分开算，不会因为全能的总补正高就全是全能
    let mut n = lib.wscal_n[r].pick(rng).unwrap_or(2.0) as usize;
    if fixed.is_some_and(|f| !f.keep && !f.full) {
        for _ in 0..20 {
            if n < 5 {
                break;
            }
            n = lib.wscal_n[r].pick(rng).unwrap_or(2.0) as usize;
        }
        n = n.min(3);
    }
    let mut stats = choose(rng, 5, n.clamp(1, 5));
    // 开局武器的主补正（最高的那项）放在职业最高的属性上（用户 2026-10-30）：抽到的几项里没有就换掉一项，抽完值再把最高的换过去
    let main = fixed.and_then(|f| usize::try_from(f.main).ok()).filter(|&m| m < 5);
    if let Some(m) = main.filter(|m| !stats.contains(m)) {
        stats[0] = m;
    }
    let mut cor = [0f32; 5];
    let mut req = [0u8; 5];
    for &k in &stats {
        let vd = lib.wscal_v.get(&(r, n as u32)).or_else(|| lib.wscal_v.get(&(r, 2)));
        cor[k] = match fixed {
            Some(f) if f.top < 1.0 => vd.and_then(|d| d.top(f.top).pick(rng)),
            _ => vd.and_then(|d| d.pick(rng)),
        }
        .unwrap_or(50.0) as f32;
        req[k] = lib.wreq.get(&(r, k)).and_then(|d| d.pick(rng)).unwrap_or(0.0).clamp(0.0, 99.0) as u8;
    }
    if let Some(m) = main {
        if let Some(&top) = stats.iter().max_by(|&&a, &&b| cor[a].total_cmp(&cor[b])) {
            if cor[top] > cor[m] {
                cor.swap(top, m);
            }
        }
    }
    if lib.wreq_sorted {
        sort_req(&stats, &cor, &mut req);
    }
    lift_level(&stats, &cor, &mut req, lib.wreq_minlvl[r]);
    if let Some(f) = fixed {
        for &k in &stats {
            req[k] = req[k].min(f.maxreq[k]);
        }
        if lib.wreq_sorted {
            // 压到职业属性以内后再按补正从高到低不升，排序照旧
            let mut prev = u8::MAX;
            for k in by_cor(&stats, &cor) {
                req[k] = req[k].min(prev);
                prev = req[k];
            }
        }
    }
    let curve = draw_curves(lib, r, typ, rng);
    // 命中异常：几种、哪几种（7 种等概率）、强度；开局武器至少 min_status 种（负数：开局的法杖、圣印记、弓、盾一个都不带，用户 2026-10-29）；全能不带
    let full = n >= 5 && lib.full_no_status || fixed.is_some_and(|f| f.min_status < 0);
    let need = if full { 0 } else { fixed.map_or(0, |f| f.min_status.max(0) as usize).min(3) };
    let m = if full { 0 } else { (lib.wstat_n[r].pick(rng).unwrap_or(0.0) as usize).max(need) };
    let mut st = [-1i32; 3];
    let status_sp = |k: usize, rng: &mut Rng| -> Option<i32> {
        let p = lib.wstat_pow.get(&(k, r)).and_then(|d| d.pick(rng))? as u32;
        let table = &lib.wstat_sp[k];
        table.get(&p).copied().or_else(|| table.iter().min_by_key(|(q, _)| q.abs_diff(p)).map(|(_, s)| *s))
    };
    let chosen = choose(rng, STATUS_N, m.min(3));
    for (j, &k) in chosen.iter().enumerate() {
        st[j] = status_sp(k, rng).unwrap_or(-1);
    }
    if need > 0 {
        // 抽到的异常这个稀有度没有强度表：按顺序换别的补够
        let mut k = rng.below(STATUS_N as u64) as usize;
        for _ in 0..STATUS_N {
            if st.iter().filter(|&&s| s > 0).count() >= need {
                break;
            }
            if let (false, Some(slot)) = (chosen.contains(&k), st.iter().position(|&s| s <= 0)) {
                if let Some(sp) = status_sp(k, rng) {
                    st[slot] = sp;
                }
            }
            k = (k + 1) % STATUS_N;
        }
    }
    // 常驻效果：法杖 / 圣印记的流派加成、盾牌抗性（保留本体的不抽）
    let keep = fixed.is_some_and(|f| f.keep);
    let mut res = if keep { [KEEP_RES; 2] } else { [-1i32; 2] };
    let mut ri = if keep { 2 } else { 0 };
    for (_name, rc) in &lib.res {
        if ri >= 2 {
            break;
        }
        if !rc.types.contains(&typ) || rc.pool.is_empty() {
            continue;
        }
        let k = rc.n[r].pick(rng).unwrap_or(0.0) as usize;
        let w: Vec<u32> = rc.pool.iter().map(|x| x.1).collect();
        for j in choose_weighted(rng, &w, k.min(2 - ri)) {
            res[ri] = rc.pool[j].0;
            ri += 1;
        }
        if ri >= 2 {
            break;
        }
    }
    let glow = if r == 3 && base.elem > 0 { lib.glow[base.elem] } else { -1 };
    // 战技：这类武器在原包里出现过的；判定换到共享层的专属战技（skillinfo 写成战灰，rr/skillgems.py）和通用战灰一样直接用，变体照本体；
    // 没换的专属战技只在同一大类（行为变体 // 100）的本体上用（行为变体改成原武器的），否则换成这类的通用战灰
    let (mut skill, mut var) = (-1, base.var);
    if keep {
        return Some(Recipe::W { base: base.id, var, skill, cor, req, curve, st, res, glow, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 });
    }
    if let Some(s) = lib.skill.get(&typ).and_then(|d| d.pick(rng)) {
        let s = s as i32;
        match lib.skillinfo.get(&s) {
            Some(i) if i.gem => skill = s,
            Some(i) if i.native >= 0 && i.native / 100 == base.var / 100 => (skill, var) = (s, skill_var(lib, base.var, s, i.native)),
            _ => {
                if let Some(d) = lib.skill_def.get(&typ).and_then(|d| d.pick(rng)) {
                    skill = d as i32;
                }
            }
        }
    }
    let spec = pick_spec(lib, tier, typ, rng);
    // 强攻放最后抽（前面的随机数顺序不变，按种子生成的开局装备只可能多一个强攻）
    let (chance, pct) = lib.wassault[r];
    let atk = if pct > 0 && rng.below(100) < chance as u64 { pct } else { 0 };
    // 属性放在强攻后面抽（前面的随机数顺序不变）：九种结果各 1/9，传说的光效跟着元素走
    let (elem, share, glow) = draw_elem(lib, typ, r, base.elem, glow, rng);
    Some(Recipe::W { base: base.id, var, skill, cor, req, curve, st, res, glow, spec, atk, ovl: false, elem, share })
}

/// 专属战技的行为变体（skillvar 行，重随的 rr/affixes.py skill_vars）：原武器有覆盖通用普攻判定的战技，配到同一大类别的本体上时用
/// 重随建的新变体（只有战技判定和原武器独有的判定），本体的轻重击退回这一大类的通用判定（以前直接用原武器的变体，连原武器的普攻判定也带过来，
/// 判定点在原武器模型上，模型对不上就打不中：碎星大剑配遗迹大剑的战技轻击没伤害，用户 2026-10-31）。本体就是原武器的照旧用原变体；
/// 以前存的配方（变体是原武器的）读档重建时也换过来。skillvar 的编号是重随按需要的战技依次分的，需要的战技少了编号会变
/// （2026-11-03 大部分专属战技的判定换到共享层后，欧赫剑舞从 398 变成 399），所以配方里记的是什么都换成这次的
fn skill_var(lib: &Lib, base_var: i32, skill: i32, var: i32) -> i32 {
    match lib.skillvar.get(&skill) {
        Some(&(orig, _)) if base_var == orig => orig,
        Some(&(_, new)) => new,
        None => var,
    }
}

/// 写进武器行的行为变体：战技当成战灰的（skillinfo 的 gem 位：通用战灰，和重随把判定换到共享层的专属战技，rr/skillgems.py，
/// 用户 2026-11-03）一律用本体自己的变体，在哪把武器上都找得到战技判定，本体的轻重击也是自己的（以前专属战技要换成原武器的变体，
/// 双持轻击没伤害）；以前存的配方里记着原武器的变体 / skillvar 变体，读档重建时也改回来。其余照 skill_var
fn row_var(lib: &Lib, base_var: i32, skill: i32, var: i32) -> i32 {
    if lib.skillinfo.get(&skill).is_some_and(|i| i.gem) {
        base_var
    } else {
        skill_var(lib, base_var, skill, var)
    }
}

/// 武器的特殊效果词条位（前缀位）：这一档出不出（wspec_p 的几率；没有就照原包的 wspec_n），出就从这类武器能出的特效里按权重挑一个
fn pick_spec(lib: &Lib, tier: usize, typ: u16, rng: &mut Rng) -> i32 {
    let r = tier.min(3);
    let on = match lib.wspec_p[tier.min(4)] {
        Some(p) => p >= 100 || rng.below(100) < p as u64,
        None => lib.wspec_n[r].pick(rng).unwrap_or(0.0) >= 1.0,
    };
    if !on {
        return -1;
    }
    let w: Vec<u32> =
        lib.wspec.iter().map(|x| if (x.types.is_empty() || x.types.contains(&typ)) && x.rar & (1 << r) != 0 { x.w } else { 0 }).collect();
    choose_weighted(rng, &w, 1).first().map_or(-1, |&k| lib.wspec[k].sp)
}

/// 有补正的几项按补正从高到低（一样高的按属性顺序）
fn by_cor(stats: &[usize], cor: &[f32; 5]) -> Vec<usize> {
    let mut v = stats.to_vec();
    v.sort_by(|&a, &b| cor[b].total_cmp(&cor[a]).then(a.cmp(&b)));
    v
}

/// 抽到的几个需求按大小重新分给有补正的几项：补正最高的拿最高的需求
fn sort_req(stats: &[usize], cor: &[f32; 5], req: &mut [u8; 5]) {
    let mut vals: Vec<u8> = stats.iter().map(|&k| req[k]).collect();
    vals.sort_unstable_by(|a, b| b.cmp(a));
    for (k, v) in by_cor(stats, cor).into_iter().zip(vals) {
        req[k] = v;
    }
}

/// 用这把武器要的等级：从全 10 起只加有需求的属性（力气按单手）
fn need_level(req: &[u8; 5]) -> u32 {
    1 + req.iter().map(|&x| (x as u32).saturating_sub(10)).sum::<u32>()
}

/// 不够 min_lvl 级：差的点数按补正比例加到有补正的几项上（先补到 10），排序不变
fn lift_level(stats: &[usize], cor: &[f32; 5], req: &mut [u8; 5], min_lvl: u32) {
    let lvl = need_level(req);
    if min_lvl == 0 || lvl >= min_lvl || stats.is_empty() {
        return;
    }
    let short = (min_lvl - lvl) as f64;
    let total: f64 = stats.iter().map(|&k| cor[k].max(0.0) as f64).sum();
    for &k in stats {
        let share = if total > 0.0 { cor[k].max(0.0) as f64 / total } else { 1.0 / stats.len() as f64 };
        req[k] = (req[k].max(10) as f64 + (short * share).ceil()).min(99.0) as u8;
    }
}

/// 鉴定偏向加点：先生成 lib.bias_n 把候选，按 够不够得着（差 g 点 × 0.5^(g/4)）×（0.1 + 补正和加点的吻合度）² 挑一把。
/// 吻合度 = Σ补正×加的点 / (Σ补正 × 加得最多的点)，0~1，加的点 = 属性 − 10（一点没加 = 0.5）；
/// 属性 = 力气、灵巧、智力、信仰、感应（力气按单手）
fn gen_weapon_for(lib: &Lib, tier: usize, stats: Option<[u32; 5]>, rng: &mut Rng) -> Option<Recipe> {
    let Some(s) = stats.filter(|_| lib.bias_n > 1) else {
        return gen_weapon(lib, tier, rng);
    };
    let cands: Vec<Recipe> = (0..lib.bias_n).filter_map(|_| gen_weapon(lib, tier, rng)).collect();
    let w: Vec<u32> = cands.iter().map(|c| (build_weight(c, &s) * 10000.0) as u32 + 1).collect();
    let j = *choose_weighted(rng, &w, 1).first()?;
    cands.into_iter().nth(j)
}

/// 定向鉴定：本体从指定类型里挑，同样偏向加点（候选都从这个类型里抽）
fn gen_weapon_for_typed(lib: &Lib, tier: usize, typ: u16, s: [u32; 5], rng: &mut Rng) -> Option<Recipe> {
    if lib.bias_n <= 1 {
        return gen_weapon_typed(lib, tier, typ, rng);
    }
    let cands: Vec<Recipe> = (0..lib.bias_n).filter_map(|_| gen_weapon_typed(lib, tier, typ, rng)).collect();
    let w: Vec<u32> = cands.iter().map(|c| (build_weight(c, &s) * 10000.0) as u32 + 1).collect();
    let j = *choose_weighted(rng, &w, 1).first()?;
    cands.into_iter().nth(j)
}

fn build_weight(rec: &Recipe, s: &[u32; 5]) -> f64 {
    let Recipe::W { cor, req, .. } = rec else {
        return 1.0;
    };
    let gap: u32 = (0..5).map(|k| (req[k] as u32).saturating_sub(s[k])).sum();
    let put: Vec<f64> = s.iter().map(|&x| x.saturating_sub(10) as f64).collect();
    let top = put.iter().copied().fold(0.0, f64::max);
    let sc: f64 = cor.iter().map(|&c| c.max(0.0) as f64).sum();
    let fit = if sc > 0.0 && top > 0.0 { (0..5).map(|k| cor[k].max(0.0) as f64 * put[k]).sum::<f64>() / (sc * top) } else { 0.5 };
    0.5f64.powf(gap as f64 / 4.0) * (0.1 + fit).powi(2)
}

/// 重抽一组补正、能力需求、补正曲线（同 gen_weapon_inner 不带约束时的抽法）。full = true 一定是全能（五项都有补正）；
/// false 不抽全能，别的项数照原版比例：抽到五项就重抽，20 次还是五项就按三项
fn draw_scaling(lib: &Lib, r: usize, typ: u16, full: bool, rng: &mut Rng) -> ([f32; 5], [u8; 5], [u8; CURVE_N]) {
    let mut n = if full { 5 } else { lib.wscal_n[r].pick(rng).unwrap_or(2.0) as usize };
    if !full {
        for _ in 0..20 {
            if n < 5 {
                break;
            }
            n = lib.wscal_n[r].pick(rng).unwrap_or(2.0) as usize;
        }
        if n >= 5 {
            n = 3;
        }
    }
    let stats = choose(rng, 5, n.clamp(1, 5));
    let mut cor = [0f32; 5];
    let mut req = [0u8; 5];
    for &k in &stats {
        let vd = lib.wscal_v.get(&(r, n as u32)).or_else(|| lib.wscal_v.get(&(r, 2)));
        cor[k] = vd.and_then(|d| d.pick(rng)).unwrap_or(50.0) as f32;
        req[k] = lib.wreq.get(&(r, k)).and_then(|d| d.pick(rng)).unwrap_or(0.0).clamp(0.0, 99.0) as u8;
    }
    if lib.wreq_sorted {
        sort_req(&stats, &cor, &mut req);
    }
    lift_level(&stats, &cor, &mut req, lib.wreq_minlvl[r]);
    (cor, req, draw_curves(lib, r, typ, rng))
}

/// 补正曲线（九种：物理 / 魔力 / 火 / 雷 / 圣 / 毒 / 出血 / 睡眠 / 发狂）。这类武器有 wcurvet 行的照它（用户 2026-11-25「照原版重做」：
/// 物理按原版这类武器连质变在内的分布，元素一律 4 号，异常 6 号，法杖 / 圣印记五种伤害同一条、按原版法器的分布），
/// 一行一组伤害种类共用一次抽到的曲线；没写到的种类（旧的定义没有 wcurvet）照旧按稀有度的 wcurve 抽
fn draw_curves(lib: &Lib, r: usize, typ: u16, rng: &mut Rng) -> [u8; CURVE_N] {
    let mut curve = [0u8; CURVE_N];
    let mut done = [false; CURVE_N];
    for (ks, d) in lib.wcurvet.get(&typ).map_or(&[][..], |v| v.as_slice()) {
        let c = d.pick(rng).unwrap_or(0.0) as u8;
        for &k in ks {
            curve[k] = c;
            done[k] = true;
        }
    }
    for k in 0..CURVE_N {
        if !done[k] {
            curve[k] = lib.wcurve.get(&(r, k)).and_then(|d| d.pick(rng)).unwrap_or(0.0) as u8;
        }
    }
    curve
}

/// 平均多少块重铸石出一把全能（用户 2026-11-09）
const REFORGE_OMNI_ODDS: u64 = 100;

/// 武器重铸石（用户 2026-10-30）：这把武器的补正、能力需求、补正曲线按它的稀有度重抽，本体、战技、异常、常驻效果、光效、特效、强攻都不变。
/// 全能（五项补正）重铸后还是全能（用户 2026-11-04），只重抽五项的值；别的每次有 1 / REFORGE_OMNI_ODDS 的概率出全能，带异常的也一样，异常保留
/// （用户 2026-11-09：只是生成时不出带异常的全能）。这一步在挑候选之前定，稀有度和加点都影响不到，没出全能就照原版比例抽项数。
/// 和鉴定一样偏向加点（stats = 力气、灵巧、智力、信仰、感应）：抽 lib.bias_n 组候选、按 build_weight 挑一组；stats = None 就直接用第一组
fn reforge_recipe(lib: &Lib, tier: usize, recipe: &Recipe, stats: Option<[u32; 5]>, rng: &mut Rng) -> Option<Recipe> {
    let &Recipe::W { base, var, skill, st, res, glow, spec, atk, cor: cor0, elem, share, .. } = recipe else {
        return None;
    };
    let r = tier.min(3);
    // 本体的武器类型（补正曲线按它抽；本体不在这次的本体表里就是 0，曲线照旧按稀有度抽）
    let typ = lib.wbase.iter().find(|b| b.id == base).map_or(0, |b| b.typ);
    let full = cor0.iter().all(|&c| c > 0.0) || rng.below(REFORGE_OMNI_ODDS) == 0;
    let n = if stats.is_some() { lib.bias_n.max(1) } else { 1 };
    let cands: Vec<Recipe> = (0..n)
        .map(|_| {
            let (cor, req, curve) = draw_scaling(lib, r, typ, full, rng);
            Recipe::W { base, var, skill, cor, req, curve, st, res, glow, spec, atk, ovl: false, elem, share }
        })
        .collect();
    let Some(s) = stats.filter(|_| cands.len() > 1) else {
        return cands.into_iter().next();
    };
    let w: Vec<u32> = cands.iter().map(|c| (build_weight(c, &s) * 10000.0) as u32 + 1).collect();
    let j = *choose_weighted(rng, &w, 1).first()?;
    cands.into_iter().nth(j)
}

fn gen_armor(lib: &Lib, tier: usize, rng: &mut Rng) -> Option<Recipe> {
    gen_armor_on(lib, tier, None, false, rng)
}

/// fixed = 本体：开局装备用；full = 强制三个防具词条位（英雄防具商店的小极品传说）
fn gen_armor_on(lib: &Lib, tier: usize, fixed: Option<u32>, full: bool, rng: &mut Rng) -> Option<Recipe> {
    gen_armor_in(lib, tier, fixed, full, None, rng)
}

/// 防具鉴定：只出这个部位（0 头 / 1 身 / 2 手 / 3 腿，用户 2026-11-09）
fn gen_armor_slot(lib: &Lib, tier: usize, slot: u8, rng: &mut Rng) -> Option<Recipe> {
    gen_armor_in(lib, tier, None, false, Some(slot), rng)
}

fn gen_armor_in(lib: &Lib, tier: usize, fixed: Option<u32>, full: bool, want: Option<u8>, rng: &mut Rng) -> Option<Recipe> {
    let r = tier.min(3);
    let base = match fixed {
        Some(id) => lib.pbase.iter().find(|b| b.id == id)?,
        None => {
            let slot = match want {
                Some(x) => x,
                None => lib.pslot[r].pick(rng)? as u8,
            };
            let bases: Vec<&PBase> = lib.pbase.iter().filter(|b| b.slot == slot).collect();
            *bases.get(rng.below(bases.len() as u64) as usize)?
        }
    };
    let can = |f: &Fam| f.r[r].is_some() && f.slots & (1 << base.slot) != 0;
    let picked: Vec<usize> = if full {
        armor_positions(lib, &[100, 100, 100], |f| if can(f) { f.armor_w[r] } else { 0 }, rng)
    } else {
        match lib.pslot_p[tier.min(4)] {
            Some(p) => armor_positions(lib, &p, |f| if can(f) { f.armor_w[r] } else { 0 }, rng),
            None => {
                let n = lib.pcount[tier.min(4)].pick(rng).unwrap_or(1.0) as usize;
                let w: Vec<u32> = lib.fams.iter().map(|f| if can(f) { f.armor_w[r] } else { 0 }).collect();
                choose_weighted(rng, &w, n.clamp(1, 3))
            }
        }
    };
    let sps: Vec<(i32, String)> = picked.iter().map(|&k| (lib.fams[k].r[r].as_ref().unwrap().sp, lib.fams[k].key.clone())).collect();
    // 词条数值随机：每条随机词条在这一档的范围里抽一个位置（固定数值的是 0；全是 0 就不存）
    let mut pos: Vec<i16> = picked.iter().map(|&k| lib.fams[k].r[r].as_ref().unwrap().val.as_ref().map_or(0, |v| v.sample(rng))).collect();
    if pos.iter().all(|&p| p == 0) {
        pos.clear();
    }
    (!sps.is_empty()).then_some(Recipe::P { base: base.id, sps, mult: None, pos })
}

/// 防具的三个位置：前缀位（p[0]%）抽一条机制类，两个后缀位（p[1]%、p[2]%）各抽一条数值类；都没掷中就补一个后缀。
/// 位置上按词条库的权重抽（每条一样、变体组平分），同一个合并组只出一个。返回词条序号（前缀在前）
fn armor_positions(lib: &Lib, p: &[u32; 3], weight: impl Fn(&Fam) -> u32, rng: &mut Rng) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let pick = |mech: bool, out: &mut Vec<usize>, rng: &mut Rng| {
        let w: Vec<u32> = lib
            .fams
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let taken = out.iter().any(|&j| j == k || (!f.group.is_empty() && lib.fams[j].group == f.group));
                if f.mech == mech && !taken { weight(f) } else { 0 }
            })
            .collect();
        if let Some(&k) = choose_weighted(rng, &w, 1).first() {
            out.push(k);
        }
    };
    let roll = |pct: u32, rng: &mut Rng| pct >= 100 || rng.below(100) < pct as u64;
    if roll(p[0], rng) {
        pick(true, &mut out, rng);
    }
    for &pct in &p[1..] {
        if roll(pct, rng) {
            pick(false, &mut out, rng);
        }
    }
    if out.is_empty() {
        pick(false, &mut out, rng);
    }
    out
}

fn gen_talisman(lib: &Lib, tier: usize, rng: &mut Rng) -> Option<Recipe> {
    gen_talisman_in(lib, tier, None, rng)
}

/// 护符鉴定：只出这个流派的词条（lib.arch_names 的序号，用户 2026-11-09）；这一档没有这个流派的护符词条就生成不了
fn gen_talisman_arch(lib: &Lib, tier: usize, arch: u8, rng: &mut Rng) -> Option<Recipe> {
    gen_talisman_in(lib, tier, Some(arch), rng)
}

fn gen_talisman_in(lib: &Lib, tier: usize, arch: Option<u8>, rng: &mut Rng) -> Option<Recipe> {
    let r = tier.min(3);
    let w: Vec<u32> = lib
        .fams
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let ok = f.r[r].as_ref().is_some_and(|x| x.tal >= 0) && arch.is_none_or(|a| lib.fam_arch.get(k) == Some(&a));
            if ok { f.tal_w[r] } else { 0 }
        })
        .collect();
    let k = *choose_weighted(rng, &w, 1).first()?;
    let fr = lib.fams[k].r[r].as_ref()?;
    Some(Recipe::A { tal: fr.tal as u32, sp: fr.sp, key: lib.fams[k].key.clone(), pos: fr.val.as_ref().map_or(0, |v| v.sample(rng)) })
}

fn generate(lib: &Lib, cat: usize, tier: usize, rng: &mut Rng) -> Option<Recipe> {
    match cat {
        0 => gen_weapon(lib, tier, rng),
        1 => gen_armor(lib, tier, rng),
        _ => gen_talisman(lib, tier, rng),
    }
}

// ------------------------------------------------------------------ 文字
fn sentence(lib: &Lib, key: &str, r: usize, lang: usize) -> String {
    lib.fam_by_key.get(key).and_then(|&k| lib.fams[k].r[r].as_ref()).map(|x| x.text[lang].clone()).unwrap_or_default()
}

/// 一件装备上这条词条的说明：词条数值随机的按记录里的位置写精确的数，固定数值的照这一档
fn sentence_at(lib: &Lib, key: &str, r: usize, pos: i16, lang: usize) -> String {
    lib.fam_by_key
        .get(key)
        .and_then(|&k| lib.fams[k].r[r].as_ref())
        .map(|x| x.val.as_ref().map_or_else(|| x.text[lang].clone(), |v| v.text(pos_f(pos), lang)))
        .unwrap_or_default()
}

/// 防具第 i 条词条的位置（没存的是 0）
fn pos_at(pos: &[i16], i: usize) -> i16 {
    pos.get(i).copied().unwrap_or(0)
}

/// 这条词条这一档是不是一期的数值随机（效果由 gear/valfx.rs 挂占位行，装备行上不挂）
fn is_val(lib: &Lib, key: &str, r: usize) -> bool {
    lib.fam_by_key.get(key).and_then(|&k| lib.fams[k].r[r].as_ref()).is_some_and(|x| x.val.as_ref().is_some_and(|v| !v.rows.is_empty()))
}

/// 这条词条这一档有没有取值定义（一期、二期都算：生成时抽位置、说明按位置写）
fn has_val(lib: &Lib, key: &str, r: usize) -> bool {
    lib.fam_by_key.get(key).and_then(|&k| lib.fams[k].r[r].as_ref()).is_some_and(|x| x.val.is_some())
}

/// 二期：身上这条词条这个稀有度的随机词条里最好的那件的位置（-1~1；同名只认最高）；身上没有这条词条这一档的生成装备是 None
pub(crate) fn val_best(key: &str, r: usize) -> Option<f32> {
    valfx::worn().into_iter().filter(|w| w.0 == key && w.1 == r).map(|w| w.2).max_by(|a, b| a.total_cmp(b))
}

/// 二期：积木引擎要覆盖的规则里的数 (这条词条的第几条规则, 字段, 精确的数)（位置 pos 的）
pub(crate) fn rule_nums(key: &str, r: usize, pos: f32) -> Vec<(usize, String, f32)> {
    let Some(v) = defs().and_then(|d| d.lib.fam_by_key.get(key).and_then(|&k| d.lib.fams[k].r[r].as_ref()).and_then(|x| x.val.as_deref())) else { return Vec::new() };
    let nums: HashMap<usize, f32> = v.nums_at(pos).into_iter().collect();
    v.rnum.iter().filter_map(|(nth, fld, a, b, j)| nums.get(j).map(|n| (*nth, fld.clone(), a + b * n))).collect()
}

/// 二期：有没有要覆盖规则里的数的词条（积木引擎每帧先看这个，没有就不查身上的装备）
pub(crate) fn rule_keys() -> &'static HashSet<String> {
    static K: OnceLock<HashSet<String>> = OnceLock::new();
    K.get_or_init(|| {
        defs().map_or_else(HashSet::new, |d| {
            d.lib.fams.iter().filter(|f| f.r.iter().flatten().any(|x| x.val.as_ref().is_some_and(|v| !v.rnum.is_empty()))).map(|f| f.key.clone()).collect()
        })
    })
}

/// 二期：专用模块 run 这一档的数（身上最好那件的位置的；身上没有这一档的生成装备、或这个模块不随机是 None，模块照原来的数）
pub(crate) fn run_val(run: &str, r: usize) -> Option<f32> {
    // 模块 → 词条序号（模块每帧都问，先建好表）
    static RUNS: OnceLock<HashMap<String, usize>> = OnceLock::new();
    let d = defs()?;
    let k = *RUNS
        .get_or_init(|| {
            let mut m = HashMap::new();
            for (k, f) in d.lib.fams.iter().enumerate() {
                for v in f.r.iter().flatten().filter_map(|x| x.val.as_deref()) {
                    for x in &v.runs {
                        m.insert(x.0.clone(), k);
                    }
                }
            }
            m
        })
        .get(run)?;
    let f = &d.lib.fams[k];
    let v = f.r[r].as_ref()?.val.as_deref()?;
    let (_, a, b, j) = v.runs.iter().find(|x| x.0 == run)?;
    let pos = val_best(&f.key, r)?;
    let n = v.nums_at(pos).into_iter().find(|x| x.0 == *j)?.1;
    Some(a + b * n)
}

fn suffix(lib: &Lib, key: &str, lang: usize) -> String {
    lib.fam_by_key.get(key).map(|&k| lib.fams[k].suffix[lang].clone()).unwrap_or_default()
}

/// 词条效果 sp 是这条词条哪个稀有度的（装备继承转过来的词条保留来源的稀有度，和装备本身的可能不同）
fn sp_rar(lib: &Lib, key: &str, sp: i32) -> Option<usize> {
    let f = &lib.fams[*lib.fam_by_key.get(key)?];
    (0..4).find(|&r| f.r[r].as_ref().is_some_and(|x| x.sp == sp))
}

/// 防具一条词条的说明：按这条词条自己的稀有度写、上这个稀有度的颜色（转过来的词条和装备的稀有度可能不同，用户 2026-10-31：
/// 用颜色区分，不再在句末注明）
fn armor_line(lib: &Lib, key: &str, sp: i32, pos: i16, r: usize, lang: usize) -> String {
    let rr = sp_rar(lib, key, sp).unwrap_or(r);
    paint(lib, rr, &sentence_at(lib, key, rr, pos, lang))
}

/// 词条说明上稀有度的颜色：用稀有度名字带的 <font color="#…">（affixes.txt 的 rar 行），每行各包一层（不让标记跨行）
fn paint(lib: &Lib, r: usize, s: &str) -> String {
    let tag = &lib.rar[r.min(3)][0];
    let open = match (tag.starts_with("<font"), tag.find('>')) {
        (true, Some(i)) => &tag[..=i],
        _ => return s.to_string(),
    };
    s.split('\n').map(|l| if l.is_empty() { String::new() } else { format!("{open}{l}</font>") }).collect::<Vec<_>>().join("\n")
}

/// 去掉 <font …> </font> 标记（稀有度名字带颜色）
fn strip_font(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// 这件装备的（名字, 简介, 说明）。简介是一行：防具写「N 个词条：后缀…」，护符写词条说明的第一行（比 info_cap 长就写「效果：后缀」）。
/// 名字（用户 2026-10-28）：机制类词条做前缀、数值类做后缀——
///   武器「稀有度 [全能·][特效] 本体名」：只加前缀（特效都是机制类；全能是五项补正，不是词条，放在特效前面）；
///   防具「稀有度 [前缀] 本体名·后缀·后缀」：第一个机制类词条做前缀，数值类最多两个做后缀，放不下的只在简介、说明里；
///   护符只有一个词条，照旧「稀有度 后缀护符」
fn compose_cap(lib: &Lib, rec: &Rec, lang: usize, info_cap: usize) -> [String; 3] {
    let r = rec.rar();
    let tag = &lib.rar[r][lang];
    let zh = lang == 0;
    match &rec.recipe {
        Recipe::W { base, skill, cor, res, spec, atk, .. } => {
            let b = lib.wbase.iter().find(|x| x.id == *base);
            let sp = lib.wspec.iter().find(|x| x.sp == *spec);
            let bname = b.map(|x| x.name[lang].clone()).unwrap_or_default();
            let stats: Vec<String> = (0..5).filter(|&k| cor[k] > 0.0).map(|k| lib.stat[k][lang].clone()).collect();
            // 前缀：全能（五项补正）、特效；别的补正看简介那一行，强攻看说明
            let mut pre: Vec<String> = Vec::new();
            if stats.len() >= 5 {
                pre.push(if zh { "全能" } else { "Versatile" }.to_string());
            }
            if let Some(x) = sp {
                pre.push(x.name[lang].clone());
            }
            let pre = if pre.is_empty() { String::new() } else { format!("{} ", pre.join(if zh { "·" } else { " " })) };
            let sk = lib.skillinfo.get(skill).map(|i| i.name[lang].clone()).unwrap_or_default();
            let mut info = match (zh, sk.is_empty()) {
                (true, false) => format!("补正：{}　战技：{sk}", stats.join("、")),
                (true, true) => format!("补正：{}", stats.join("、")),
                (false, false) => format!("Scaling: {} · Skill: {sk}", stats.join(", ")),
                (false, true) => format!("Scaling: {}", stats.join(", ")),
            };
            if let Some(x) = sp {
                info += &if zh { format!("　特效：{}", x.name[0]) } else { format!(" · Special: {}", x.name[1]) };
            }
            let mut lines: Vec<String> = Vec::new();
            if *atk > 0 {
                lines.push(if zh { format!("强攻：基础攻击力＋{atk}％") } else { format!("Assault: base attack +{atk}%") });
            }
            lines.extend(
                res.iter()
                    .filter(|&&s| s > 0)
                    .filter_map(|s| lib.res.iter().flat_map(|x| x.1.pool.iter()).find(|p| p.0 == *s).map(|p| p.2[lang].clone())),
            );
            // 常驻效果（法杖 / 圣印记的流派加成、盾牌抗性）：双持时同名效果会重复生效（2026-11-20 实机确认，保留这个行为），
            // 以前这里补的「同名效果不叠加（双持时）」与实际不符，已删（见更新日志 2026-11-05 那条）
            if let Some(x) = sp {
                lines.push(x.text[lang].clone());
            }
            [format!("{tag} {pre}{bname}"), info, lines.join("\n")]
        }
        Recipe::P { base, sps, pos, .. } => {
            let bname = lib.pbase.iter().find(|x| x.id == *base).map(|x| x.name[lang].clone()).unwrap_or_default();
            let lines: Vec<String> = sps.iter().enumerate().map(|(i, x)| armor_line(lib, &x.1, x.0, pos_at(pos, i), r, lang)).filter(|s| !s.is_empty()).collect();
            let mech = |k: &str| lib.fam_by_key.get(k).is_some_and(|&i| lib.fams[i].mech);
            // 英文的后缀名（原包的）有的全小写：放进名字时每个词首字母大写
            let word = |k: &str| {
                let s = suffix(lib, k, lang);
                if zh {
                    return s;
                }
                s.split(' ').map(|w| w.chars().take(1).flat_map(char::to_uppercase).chain(w.chars().skip(1)).collect::<String>()).collect::<Vec<_>>().join(" ")
            };
            let pre = sps.iter().find(|x| mech(&x.1)).map_or(String::new(), |x| format!("{} ", word(&x.1)));
            let post: Vec<String> = sps.iter().filter(|x| !mech(&x.1)).take(2).map(|x| word(&x.1)).collect();
            let post = match (post.is_empty(), zh) {
                (true, _) => String::new(),
                (false, true) => format!("·{}", post.join("·")),
                (false, false) => format!(" of {}", post.join(" and ")),
            };
            let name = format!("{tag} {pre}{bname}{post}");
            let sfx: Vec<String> = sps.iter().map(|x| suffix(lib, &x.1, lang)).collect();
            let info = if zh { format!("{} 个词条：{}", sps.len(), sfx.join("、")) } else { format!("{} effect(s): {}", sps.len(), sfx.join(", ")) };
            [name, info, lines.join("\n")]
        }
        Recipe::A { key, pos, .. } => {
            let s = suffix(lib, key, lang);
            let t = sentence_at(lib, key, r, *pos, lang);
            let name = if zh { format!("{tag} {s}护符") } else { format!("{tag} Amulet of {s}") };
            let first = t.split('\n').next().unwrap_or_default();       // 简介一行：说明的第一行
            let info = if first.encode_utf16().count() <= info_cap {
                first.to_string()
            } else if zh {
                format!("效果：{s}（详见说明）")
            } else {
                format!("Effect: {s} (see description)")
            };
            [name, info, paint(lib, r, &t)]
        }
    }
}

// ------------------------------------------------------------------ 参数行
/// 读一个 4 字节整数字段（i / I）
fn get_i32(lib: &Lib, cat: char, row: &[u8], name: &str) -> Option<i32> {
    let f = lib.fields.get(&(cat, name.to_string()))?;
    matches!(f.fmt, b'i' | b'I').then(|| row.get(f.off..f.off + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap())))?
}

/// 读一个 2 字节字段（H，比如武器的基础攻击力；h 的强化类型这些都是正数，也照无符号读）
fn get_u16(lib: &Lib, cat: char, row: &[u8], name: &str) -> Option<u16> {
    let f = lib.fields.get(&(cat, name.to_string()))?;
    matches!(f.fmt, b'H' | b'h').then(|| row.get(f.off..f.off + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap())))?
}

/// frames.txt（启动器合成菜单包时从 data\menu\dr_frames.txt 抄过来；合成失败就没有）：一行「原图标 稀有度 带框的图标」
fn parse_frames(text: &str) -> HashMap<(u16, usize), u16> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<u32> = l.split_whitespace().map(|x| x.parse().ok()).collect::<Option<_>>()?;
            (f.len() == 3 && f[1] < 4 && f[0] <= u16::MAX as u32 && f[2] <= u16::MAX as u32).then(|| ((f[0] as u16, f[1] as usize), f[2] as u16))
        })
        .collect()
}

/// frames.txt 的「custom 名字 图标」行：make_frames.py 画的自制道具图标（菜单包合成成功才有这个文件，没有就照重随写的原版图标）
fn parse_custom_icons(text: &str) -> HashMap<String, u16> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            match f.as_slice() {
                ["custom", name, icon] => Some((name.to_string(), icon.parse().ok()?)),
                _ => None,
            }
        })
        .collect()
}

/// 防具的八种减伤（存的是受伤倍率：0.9 = 减伤 10%）
const CUT_FIELDS: [&str; 8] = [
    "neutralDamageCutRate", "slashDamageCutRate", "blowDamageCutRate", "thrustDamageCutRate",
    "magicDamageCutRate", "fireDamageCutRate", "thunderDamageCutRate", "darkDamageCutRate",
];

fn get_f32(lib: &Lib, cat: char, row: &[u8], name: &str) -> Option<f32> {
    let f = lib.fields.get(&(cat, name.to_string()))?;
    (f.fmt == b'f').then(|| row.get(f.off..f.off + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap())))?
}

/// 防具减伤倍率：截断正态（均值 = 区间中点，区间 = ±2σ，抽到外面重抽）。随机数按这件的记录（容器序号、档、本体、词条）定，
/// 同一件每次读档都一样，记录格式不用改，以前拿到的也有
fn armor_mult(lo: f64, hi: f64, rec: &Rec, base: u32, sps: &[(i32, String)]) -> f64 {
    let mut h = 0x5EED_A12A_0DEF_0001u64 ^ (rec.idx as u64) << 24 ^ (rec.tier as u64) << 56 ^ base as u64;
    for (sp, _) in sps {
        h = h.rotate_left(13) ^ *sp as u32 as u64;
    }
    trunc_normal(lo, hi, h)
}

/// 法器倍率（百分数，100 = 原版；catalyst_base）：能施法的本体（wcast）按稀有度区间抽截断正态（同防具减伤），按记录（容器序号、档、本体）定，
/// 同一件每次一样、重铸不变；参数行的五项补正值乘它 ÷ 100（build_row），面板上的补正、法术补正都跟着变。不是法器、没配区间的 None
/// 全能（五项都有补正）的补正倍率（Lib::womni，默认 1）：参数行的补正 = 配方 × 法器倍率 × 这个
fn omni_mult(lib: &Lib, cor: &[f32; 5]) -> f64 {
    if cor.iter().all(|&c| c > 0.0) {
        lib.womni.unwrap_or(1.0)
    } else {
        1.0
    }
}

fn cast_base(lib: &Lib, rec: &Rec) -> Option<f64> {
    let Recipe::W { base, .. } = &rec.recipe else { return None };
    lib.wcast.get(base)?;
    let (lo, hi) = lib.cbase[rec.rar()]?;
    Some(trunc_normal(lo, hi, 0x5EED_CA57_0BA5_E001u64 ^ (rec.idx as u64) << 24 ^ (rec.tier as u64) << 56 ^ *base as u64))
}

/// 截断正态：均值 = 区间中点，区间 = ±2σ，抽到外面重抽（32 次都在外面取均值）
fn trunc_normal(lo: f64, hi: f64, seed: u64) -> f64 {
    let mut rng = Rng(seed);
    let (mu, sd) = ((lo + hi) / 2.0, (hi - lo) / 4.0);
    if sd <= 0.0 {
        return mu;
    }
    for _ in 0..32 {
        let u1 = ((rng.next() >> 11) as f64 + 1.0) / (1u64 << 53) as f64; // (0, 1]
        let u2 = (rng.next() >> 11) as f64 / (1u64 << 53) as f64;
        let m = mu + sd * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
        if (lo..=hi).contains(&m) {
            return m;
        }
    }
    mu
}

/// 盾牌随机的一项：在排好序的原版值里取 [lo, hi] 之间的一个分位（线性插值）；按记录定种子，同一件每次一样、各项不一样
fn guard_pick(vs: &[f32], (lo, hi): (f64, f64), rec: &Rec, base: u32, k: usize) -> Option<f64> {
    let n = vs.len();
    if n == 0 {
        return None;
    }
    let mut rng = Rng(0x5EED_6A2D_5111_E1D0 ^ (rec.idx as u64) << 24 ^ (rec.tier as u64) << 56 ^ base as u64 ^ (k as u64 + 1) << 8);
    rng.next();
    let u = lo + (hi - lo) * ((rng.next() >> 11) as f64 / (1u64 << 53) as f64);
    let p = u.clamp(0.0, 1.0) * (n - 1) as f64;
    let i = (p.floor() as usize).min(n - 1);
    let j = (i + 1).min(n - 1);
    Some(vs[i] as f64 + (vs[j] - vs[i]) as f64 * (p - i as f64))
}

/// 各类装备的图标字段（防具分男女两个）
const ICON_FIELDS: [&[&str]; NCAT] = [&["iconId"], &["iconIdM", "iconIdF"], &["iconId"]];

/// 强攻乘的五项基础攻击力
const ATTACK_BASE: [&str; 5] = ["attackBasePhysics", "attackBaseMagic", "attackBaseFire", "attackBaseThunder", "attackBaseDark"];

/// 武器属性随机（用户 2026-11-10）：九种结果各 1/9——纯物理、物理＋魔力 / 火 / 雷 / 圣（元素占 lo~hi %）、纯魔力 / 火 / 雷 / 圣。
/// 拆成两种属性时各自对防御结算、会多扣一点：总基础攻击力 ×(1 + coef × s × (1 − s))（五五开约 ×1.075）；
/// 纯元素：Boss 对元素普遍比物理多减伤 10 个百分点左右，× pure（约 1.08）。原版带元素的本体先按同一套公式折回纯物理再分
#[derive(Debug, Clone, PartialEq)]
struct WElem {
    on: bool,
    lo: u8,
    hi: u8,
    coef: f64,
    pure: f64,
    /// 不抽的武器类型（法杖、圣印记、弓弩、火把、调香瓶）
    skip: Vec<u16>,
}

impl Default for WElem {
    fn default() -> Self {
        WElem { on: false, lo: 30, hi: 60, coef: 0.3, pure: 1.08, skip: Vec::new() }
    }
}

/// 配方 elem 的元素序号（1 魔力 2 火 3 雷 4 圣；纯物理 / 照本体 0）
fn elem_index(elem: u8) -> usize {
    match elem {
        2..=5 => (elem - 1) as usize,
        6..=9 => (elem - 5) as usize,
        _ => 0,
    }
}

/// 五项基础攻击力按配方的属性重新分配：先折回纯物理（原版带元素的按同一套补偿反算），再按 elem / share 分
fn elem_attack(w: &WElem, base: [f64; 5], elem: u8, share: u8) -> [f64; 5] {
    let total: f64 = base.iter().sum();
    if elem == 0 || total <= 0.0 {
        return base;
    }
    let e0 = total - base[0];
    let p_eq = if e0 <= 0.0 {
        total
    } else if base[0] <= 0.0 {
        total / w.pure
    } else {
        let s0 = e0 / total;
        total / (1.0 + w.coef * s0 * (1.0 - s0))
    };
    let mut out = [0.0; 5];
    let k = elem_index(elem);
    match elem {
        2..=5 => {
            let s = (share.clamp(1, 99) as f64) / 100.0;
            let tot = p_eq * (1.0 + w.coef * s * (1.0 - s));
            out[0] = tot * (1.0 - s);
            out[k] = tot * s;
        }
        6..=9 => out[k] = p_eq * w.pure,
        _ => out[0] = p_eq,
    }
    out
}

/// 传说武器的属性光效：按配方的元素（elem = 0 照本体的属性）
fn elem_glow(lib: &Lib, r: usize, elem: u8, base_elem: usize) -> i32 {
    let e = if elem == 0 { base_elem } else { elem_index(elem) };
    if r == 3 && e > 0 { lib.glow[e.min(4)] } else { -1 }
}

fn put(lib: &Lib, cat: char, row: &mut [u8], name: &str, v: f64) {
    let Some(f) = lib.fields.get(&(cat, name.to_string())) else { return };
    let o = f.off;
    match f.fmt {
        b'f' if o + 4 <= row.len() => row[o..o + 4].copy_from_slice(&(v as f32).to_le_bytes()),
        b'i' if o + 4 <= row.len() => row[o..o + 4].copy_from_slice(&(v as i32).to_le_bytes()),
        b'I' if o + 4 <= row.len() => row[o..o + 4].copy_from_slice(&(v as u32).to_le_bytes()),
        b'H' if o + 2 <= row.len() => row[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes()),
        b'h' if o + 2 <= row.len() => row[o..o + 2].copy_from_slice(&(v as i16).to_le_bytes()),
        b'B' | b'b' if o < row.len() => {
            if f.bit >= 0 {
                let m = 1u8 << f.bit;
                row[o] = if v != 0.0 { row[o] | m } else { row[o] & !m };
            } else {
                row[o] = v as i64 as u8;
            }
        }
        _ => {}
    }
}

/// 强化上限：换成了重随建的强化类型（最高 +rmax）的武器，originEquipWep(rmax+1)~25 写 -1，强满后铁匠的强化列表里就没有这把了
/// （原版灵药武器 +10 封顶也是这样：11~25 是 -1；只靠强化表少行不行，游戏照样让强 +6）。
/// keep = 容器留的字段，武器就是 originEquipWep0~25 按等级排（rr/containers.py），这里在留的字段写回之后改。
/// 1~rmax 级照第 0 级（指自己）：本体原版能强化的级数不到 rmax 的（陨石杖原版不能强化），开局装备 / 追忆武器照本体复制的行这几级是 -1
fn cap_origin(lib: &Lib, keep: &[(usize, usize)], base_row: &[u8], row: &mut [u8]) {
    let c = CAT_CODE[0];
    let swapped = get_u16(lib, c, row, "reinforceTypeId") != get_u16(lib, c, base_row, "reinforceTypeId");
    if lib.rmax == 0 || !swapped || keep.len() != 26 || keep.iter().any(|&(o, l)| l != 4 || o + 4 > row.len()) {
        return;
    }
    let own: [u8; 4] = row[keep[0].0..keep[0].0 + 4].try_into().unwrap();
    for (k, &(o, _)) in keep.iter().enumerate() {
        if k > lib.rmax as usize {
            row[o..o + 4].copy_from_slice(&(-1i32).to_le_bytes());
        } else if k > 0 && i32::from_le_bytes(own) != -1 {
            row[o..o + 4].copy_from_slice(&own);
        }
    }
}

/// 照本体整行 + 配方改的字段（不含容器自己留的字段）。old：这把已经强化到新上限以上（之前拿到的），强化照原版
fn build_row(lib: &Lib, rec: &Rec, base_row: &[u8], old: bool) -> Vec<u8> {
    let mut row = base_row.to_vec();
    let r = rec.rar();
    let c = CAT_CODE[rec.cat];
    put(lib, c, &mut row, "sellValue", lib.sell[rec.cat][r] as f64);
    put(lib, c, &mut row, "rarity", r as f64);
    match &rec.recipe {
        Recipe::W { base, var, skill, cor, req, curve, st, res, glow, spec, atk, elem, share, .. } => {
            // 盾牌：五种属性的防御时减伤、防御强度各自在原版同类盾里按稀有度的分位区间抽（同一件每次一样；强化照旧乘）
            if let (Some(t), Some(q)) = (get_u16(lib, c, &row, "wepType"), lib.wguard_q[r]) {
                if let Some(g) = lib.wguard.get(&t) {
                    for (k, nm) in GUARD_F.iter().enumerate() {
                        if let Some(v) = guard_pick(&g[k], q, rec, *base, k) {
                            put(lib, c, &mut row, nm, if k == 5 { v.round() } else { (v * 10.0).round() / 10.0 });
                        }
                    }
                }
            }
            // 属性：五项基础攻击力按配方重新分配（在强攻之前，强攻照旧五项一起乘）
            if *elem > 0 {
                let mut a = [0.0f64; 5];
                for (k, nm) in ATTACK_BASE.iter().enumerate() {
                    a[k] = get_u16(lib, c, &row, nm).unwrap_or(0) as f64;
                }
                for (k, v) in elem_attack(&lib.welem, a, *elem, *share).iter().enumerate() {
                    put(lib, c, &mut row, ATTACK_BASE[k], v.round().min(u16::MAX as f64));
                }
            }
            // 强攻 × 轻 / 重武器倍率（按本体的武器类型）
            let heavy = get_u16(lib, c, &row, "wepType").and_then(|t| lib.wheavy.get(&t).copied()).unwrap_or(1.0);
            let mult = (1.0 + *atk as f64 / 100.0) * heavy;
            if mult != 1.0 {
                for nm in ATTACK_BASE {
                    if let Some(v) = get_u16(lib, c, &row, nm) {
                        put(lib, c, &mut row, nm, (v as f64 * mult).round().min(u16::MAX as f64));
                    }
                }
            }
            // 致命一击：面板值（100 + throwAtkRate）按稀有度乘，短剑这种原本高的照比例更高
            let cm = lib.wcrit[r];
            if cm > 0.0 && cm != 1.0 {
                if let Some(v) = get_u16(lib, c, &row, "throwAtkRate") {
                    put(lib, c, &mut row, "throwAtkRate", ((100.0 + v as i16 as f64) * cm).round() - 100.0);
                }
            }
            const ST: [&str; 5] = ["Strength", "Agility", "Magic", "Faith", "Luck"];
            const CV: [&str; CURVE_N] = ["Physics", "Magic", "Fire", "Thunder", "Dark", "Poison", "Blood", "Sleep", "Madness"];
            // 法器：补正值 × 法器倍率；全能再 × 全能补正倍率（配方里存的还是没乘的，重铸 / 超限照配方算）
            let cm = cast_base(lib, rec).map_or(1.0, |b| b / 100.0) * omni_mult(lib, cor);
            for k in 0..5 {
                put(lib, c, &mut row, &format!("correct{}", ST[k]), cor[k] as f64 * cm);
                put(lib, c, &mut row, &format!("proper{}", ST[k]), req[k] as f64);
            }
            for k in 0..CURVE_N {
                put(lib, c, &mut row, &format!("correctType_{}", CV[k]), curve[k] as f64);
            }
            put(lib, c, &mut row, "attackElementCorrectId", lib.element_table as f64);
            for k in 0..3 {
                put(lib, c, &mut row, &format!("spEffectBehaviorId{k}"), st[k] as f64);
                // 面板「附加效果」的文字跟着异常换（本体原来的出血文字不能留着）
                let msg = (0..STATUS_N).find(|&j| st[k] > 0 && lib.wstat_sp[j].values().any(|&x| x == st[k])).map_or(-1, |j| lib.wstat_msg[j]);
                put(lib, c, &mut row, &format!("spEffectMsgId{k}"), msg as f64);
            }
            if res[0] == KEEP_RES {
                // 照本体（追忆的成品传说）：本体自带的常驻效果不动，光效只放在空着的槽里
                if get_i32(lib, c, &row, "residentSpEffectId2").is_some_and(|v| v <= 0) {
                    put(lib, c, &mut row, "residentSpEffectId2", *glow as f64);
                }
            } else {
                put(lib, c, &mut row, "residentSpEffectId", res[0] as f64);
                put(lib, c, &mut row, "residentSpEffectId1", if *spec > 0 { *spec } else { res[1] } as f64);
                put(lib, c, &mut row, "residentSpEffectId2", *glow as f64);
            }
            if *skill > 0 {
                put(lib, c, &mut row, "swordArtsParamId", *skill as f64);
            }
            let base_var = get_i32(lib, c, &row, "behaviorVariationId").unwrap_or(*var);
            put(lib, c, &mut row, "behaviorVariationId", row_var(lib, base_var, *skill, *var) as f64);
            put(lib, c, &mut row, "disableGemAttr", 1.0); // 不能质变（质变后的编号没有这一行）
            put(lib, c, &mut row, "isEnhance", 1.0); // 生成武器都能附魔（原版武器不改）
            // 都能换战灰（用户 2026-11-01：原版不能换的失色 / 专属武器也要能换）。通用战灰的判定在本体变体下找不到时游戏退回同一大类的
            // 通用判定（原版就有 64 把能换战灰的武器自己的变体下没有战技行）；取下战灰回到这把自己的战技
            put(lib, c, &mut row, "gemMountType", 2.0);
            // 强化：按本体的原版强化类型、稀有度、全能换成重随建的（最高 +5、每级 1 块石头）；本体不能强化的没有对应，照原版
            let omni = cor.iter().all(|&x| x > 0.0);
            if let Some(&t) = get_u16(lib, c, &row, "reinforceTypeId").and_then(|t| lib.reinf.get(&(t, r, omni))).filter(|_| !old) {
                put(lib, c, &mut row, "reinforceTypeId", t as f64);
                put(lib, c, &mut row, "materialSetId", lib.rmat[r] as f64);
            }
        }
        Recipe::P { base, sps, mult, .. } => {
            // 词条数值随机的词条不在装备行上挂（gear/valfx.rs 按身上的装备挂写好精确数值的占位行）
            for (k, nm) in ["residentSpEffectId", "residentSpEffectId2", "residentSpEffectId3"].iter().enumerate() {
                let v = sps.get(k).filter(|x| !is_val(lib, &x.1, sp_rar(lib, &x.1, x.0).unwrap_or(r))).map_or(-1, |x| x.0);
                put(lib, c, &mut row, nm, v as f64);
            }
            // 减伤倍率：八种减伤的百分比都乘同一个数（受伤倍率 v → 1 - (1 - v) × m，最多减 90%）；超限过的用记录里存的倍率
            if let Some(m) = mult.or_else(|| lib.pdef[r].map(|(lo, hi)| armor_mult(lo, hi, rec, *base, sps))) {
                for nm in CUT_FIELDS {
                    if let Some(v) = get_f32(lib, c, &row, nm) {
                        put(lib, c, &mut row, nm, 1.0 - ((1.0 - v as f64) * m).min(0.9));
                    }
                }
            }
        }
        // 词条数值随机：护符行不挂（gear/valfx.rs 挂占位行）；固定数值的照模板
        Recipe::A { sp, key, .. } => {
            let rr = sp_rar(lib, key, *sp).unwrap_or(r);
            if is_val(lib, key, rr) {
                put(lib, c, &mut row, "refId", -1.0);
                // 链上别的链头放在常驻效果栏里的（霜铠的第二个监听）也不挂，valfx 挂它的占位行
                let chain: Vec<i32> = lib.fam_by_key.get(key.as_str()).and_then(|&k| lib.fams[k].r[rr].as_ref()).and_then(|x| x.val.as_ref()).map_or(Vec::new(), |v| v.rows.clone());
                for nm in ["residentSpEffectId1", "residentSpEffectId2", "residentSpEffectId3", "residentSpEffectId4"] {
                    if get_i32(lib, c, &row, nm).is_some_and(|v| v > 0 && chain.contains(&v)) {
                        put(lib, c, &mut row, nm, -1.0);
                    }
                }
            }
        }
    }
    frame_row(lib, rec.cat, &mut row, r);
    row
}

/// 稀有度边框：图标换成这个稀有度带框的那份（没有就照原版图标；已经带框的查不到，照旧）。换了几个
fn frame_row(lib: &Lib, cat: usize, row: &mut [u8], r: usize) -> usize {
    let c = CAT_CODE[cat];
    let mut n = 0;
    for nm in ICON_FIELDS[cat] {
        if let Some(&v) = get_u16(lib, c, row, nm).and_then(|i| lib.ficon.get(&(i, r))) {
            put(lib, c, row, nm, v as f64);
            n += 1;
        }
    }
    n
}

/// 新词条饰品图标的虚编号（make_frames.py ART_LO / ART_HI，codex_icons.json 里写的）：游戏里没有这个图标，只在 frames.txt 里对到各稀有度带框的
const ART_LO: u16 = 45000;
const ART_HI: u16 = 50000;

/// 这个词条这一档有自己画的饰品图标（famicon 是虚编号、frames.txt 里有带框的）就给那个带框的图标
fn art_icon(lib: &Lib, key: &str, r: usize) -> Option<u16> {
    let b = *lib.famicon.get(key)?.get(r)?;
    (ART_LO..ART_HI).contains(&b).then(|| lib.ficon.get(&(b, r)).copied())?
}

/// 护符模板（词条库每种词条每个稀有度一件，EquipParamAccessory 9000 起）的图标换成带稀有度边框的：开局遗物、暗店护符直接用模板行、
/// 不走 build_row，原来没边框（用户 2026-10-29 报的「稀有 天矛突袭护符」）。容器护符抄模板行，抄来的已经带框，build_row 查不到就照用。
/// 参数没载入（一行模板都找不到）就每 30 帧再试，试 START_MAX_TRIES 次
fn frame_templates(d: &Defs, st: &mut St) {
    if st.tal_frames || d.lib.ficon.is_empty() {
        return;
    }
    st.tal_wait = st.tal_wait.wrapping_add(1);
    if st.tal_wait % 30 != 1 {
        return;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let (mut seen, mut miss, mut n) = (0, 0, 0);
    for fam in &d.lib.fams {
        for (r, fr) in fam.r.iter().enumerate() {
            let Some(fr) = fr.as_ref().filter(|f| f.tal > 0) else { continue };
            match param_mut::<EquipParamAccessory>(repo, fr.tal as u32) {
                Some(row) => {
                    seen += 1;
                    // 新词条有自己画的饰品图标（用户 2026-11-11）就换那个，别的照原版图标加边框
                    match art_icon(&d.lib, &fam.key, r) {
                        Some(icon) => {
                            put(&d.lib, 'a', row, "iconId", icon as f64);
                            n += 1;
                        }
                        None => n += frame_row(&d.lib, 2, row, r).min(1),
                    }
                }
                None => miss += 1,
            }
        }
    }
    st.tal_tries += 1;
    if seen == 0 && st.tal_tries < START_MAX_TRIES {
        return;
    }
    st.tal_frames = true;
    // 融锻之心（重随写的是龙心脏的图标）、武器重铸石（原版砥石刀的图标，不在我们的图集里、装备工坊界面画不出来）：菜单包里有自制的就换上
    let custom = [
        ("融锻之心", "fusion_heart", d.items.inherit.map(|i| i.goods)),
        ("武器重铸石", "reforge_stone", d.items.reforge.map(|r| r.goods)),
        ("超限锻造石（武器）", "overlimit_w", d.items.overlimit.map(|o| o.gw)),
        ("超限锻造石（防具）", "overlimit_a", d.items.overlimit.map(|o| o.ga)),
        ("许愿石", "wish_stone", crate::wishstone::goods()),
    ];
    for (what, name, goods) in custom {
        let (Some(goods), Some(&icon)) = (goods.filter(|&g| g > 0), d.lib.cicon.get(name)) else { continue };
        match repo.get_mut::<EquipParamGoods>(goods) {
            Some(row) => {
                row.set_icon_id(icon);
                log(&format!("装备工坊：{what}（道具 {goods}）换上自制图标 {icon}"));
            }
            None => log(&format!("装备工坊：{what}（道具 {goods}）不在游戏参数里，图标没换")),
        }
    }
    log(&format!(
        "新装备系统：护符模板 {n} 件换上稀有度边框图标（找到 {seen} 件{}）",
        if miss > 0 { format!("，{miss} 件不在游戏参数里") } else { String::new() }
    ));
}

/// 词条的后缀名（中文, 英文）和各稀有度的效果行（魇化 nightmare.rs 用）
pub(crate) fn fam_info(key: &str) -> Option<([String; 2], [Option<i32>; 4])> {
    let d = defs()?;
    let f = d.lib.fams.get(*d.lib.fam_by_key.get(key)?)?;
    Some((f.suffix.clone(), std::array::from_fn(|r| f.r[r].as_ref().map(|x| x.sp))))
}

fn base_of(rec: &Rec) -> u32 {
    match &rec.recipe {
        Recipe::W { base, .. } | Recipe::P { base, .. } => *base,
        Recipe::A { tal, .. } => *tal,
    }
}

fn param_bytes<P: SoloParam>(repo: &SoloParamRepository, id: u32) -> Option<Vec<u8>> {
    let r = repo.get::<P>(id)?;
    let n = std::mem::size_of::<P::StructType>();
    Some(unsafe { std::slice::from_raw_parts(r as *const P::StructType as *const u8, n) }.to_vec())
}

fn param_mut<P: SoloParam>(repo: &mut SoloParamRepository, id: u32) -> Option<&'static mut [u8]> {
    let r = repo.get_mut::<P>(id)?;
    let n = std::mem::size_of::<P::StructType>();
    Some(unsafe { std::slice::from_raw_parts_mut(r as *mut P::StructType as *mut u8, n) })
}

fn row_size(cat: usize) -> usize {
    match cat {
        0 => std::mem::size_of::<<EquipParamWeapon as SoloParam>::StructType>(),
        1 => std::mem::size_of::<<EquipParamProtector as SoloParam>::StructType>(),
        _ => std::mem::size_of::<<EquipParamAccessory as SoloParam>::StructType>(),
    }
}

/// 改写容器这一行：本体整行 + 配方，容器自己留的字段从第一次改之前的样子取
fn write_row(d: &Defs, st: &mut St, rec: &Rec) -> Result<(), String> {
    let ct = &d.items.cont[rec.cat];
    if row_size(rec.cat) != ct.size {
        return Err(format!("{}参数一行的大小对不上（游戏 {}，定义 {}）", CAT_ZH[rec.cat], row_size(rec.cat), ct.size));
    }
    let repo = unsafe { SoloParamRepository::instance_mut() }.map_err(|_| "参数还没载入".to_string())?;
    let base = base_of(rec);
    let src = match rec.cat {
        0 => param_bytes::<EquipParamWeapon>(repo, base),
        1 => param_bytes::<EquipParamProtector>(repo, base),
        _ => param_bytes::<EquipParamAccessory>(repo, base),
    }
    .ok_or_else(|| format!("本体 {base} 不在游戏参数里"))?;
    let id = rec_id(&d.items, rec);
    let dst = match rec.cat {
        0 => param_mut::<EquipParamWeapon>(repo, id),
        1 => param_mut::<EquipParamProtector>(repo, id),
        _ => param_mut::<EquipParamAccessory>(repo, id),
    }
    .ok_or_else(|| format!("容器 {id} 不在游戏参数里（这次不是用重随的模式启动的？）"))?;
    let keep = st.keep.entry((rec.cat, rec.idx)).or_insert_with(|| ct.keep.iter().filter(|(o, l)| o + l <= dst.len()).map(|&(o, l)| (o, dst[o..o + l].to_vec())).collect());
    let mut new = build_row(&d.lib, rec, &src, rec.cat == 0 && st.over_max.contains(&rec.idx));
    for (o, v) in keep.iter() {
        new[*o..*o + v.len()].copy_from_slice(v);
    }
    if rec.cat == 0 {
        cap_origin(&d.lib, &ct.keep, &src, &mut new);
    }
    dst.copy_from_slice(&new);
    // 联机时不和别人共享掉落（原版的 disableMultiDropShare，联机计划 13.7）：队友捡到的只是容器编号，配方在你这边，
    // 他那边是别的东西或占位品。要给队友用物品菜单的「送给队友」。单机没有影响
    unsafe {
        use eldenring::param::{EQUIP_PARAM_ACCESSORY_ST, EQUIP_PARAM_PROTECTOR_ST, EQUIP_PARAM_WEAPON_ST};
        match rec.cat {
            0 => (*(dst.as_mut_ptr() as *mut EQUIP_PARAM_WEAPON_ST)).set_disable_multi_drop_share(true),
            1 => (*(dst.as_mut_ptr() as *mut EQUIP_PARAM_PROTECTOR_ST)).set_disable_multi_drop_share(true),
            _ => (*(dst.as_mut_ptr() as *mut EQUIP_PARAM_ACCESSORY_ST)).set_disable_multi_drop_share(true),
        }
    }
    Ok(())
}

pub(crate) use gift::{gift_in, gift_result};

// ------------------------------------------------------------------ 文字（游戏取文字时换成这里交的，msghook.rs）
// 以前扫整个进程的内存找占位、原地改写（扫到的多是临时副本，写进已经释放的内存会写坏游戏的堆，2026-11-18 两位玩家的崩溃），
// 现在只把文字交给取文字钩子，不写游戏的内存

/// 新文字放进 cap 个字：截到 cap 个字（不拆开代理对），短的后面补 0
fn fit_cap(text: &str, cap: usize) -> Vec<u16> {
    let mut u: Vec<u16> = text.encode_utf16().take(cap).collect();
    if u.len() == cap && u.last().is_some_and(|c| (0xD800..0xDC00).contains(c)) {
        u.pop();
    }
    u.resize(cap, 0);
    u
}

#[derive(Default)]
struct Txt {
    /// 确认框文字上次交给钩子的那一套（没变就不再交）
    confirm_last: String,
    /// 开局装备改好了 / 进了存档，还没按记录交一遍文字
    fresh: bool,
}

static TXT: Mutex<Option<Txt>> = Mutex::new(None);

/// 一件装备的三段文字交给取文字钩子（中英文各一份）：返回交了几条
fn write_texts(d: &Defs, rec: &Rec) -> usize {
    let id = rec_id(&d.items, rec);
    let tx = [compose_cap(&d.lib, rec, 0, d.items.cap[1]), compose_cap(&d.lib, rec, 1, d.items.cap[1])];
    (0..3).map(|field| write_field(d, id, field, [&tx[0][field], &tx[1][field]])).sum()
}

/// 占位（编号, 字段）的文字：各语言取 by_lang，截到这个字段的长度；返回交了几条
fn write_field(d: &Defs, id: u32, field: usize, by_lang: [&str; 2]) -> usize {
    let cap = d.items.cap[field];
    for (lang, t) in by_lang.iter().enumerate() {
        crate::msghook::set_item(id, field as u8, lang, &fit_cap(t, cap));
    }
    by_lang.len()
}

/// 文字要重新交一遍（开局装备改好了、进了存档）：下一帧由 start_texts / texts_tick 交
fn texts_fresh() {
    TXT.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Txt::default).fresh = true;
}

// ------------------------------------------------------------------ 状态
struct Pending {
    t0: Instant,
    off_at: Option<Instant>,
    /// (产出, 换掉的未鉴定道具)
    items: Vec<(Made, u32)>,
}

impl Pending {
    fn new(items: Vec<(Made, u32)>) -> Pending {
        Pending { t0: Instant::now(), off_at: None, items }
    }
}

#[derive(Default)]
struct St {
    who: Option<(i32, String)>,
    frames: u32,
    /// 进存档时接过来的记录是哪来的（格子的文件 / 旧记录里的一份）、几件；没接（格子里没有这个名字的）是 None。刚建的角色要把它挪开
    inherit: Option<(String, usize)>,
    /// 已经按「刚建的角色」处理过的身份编号（who::fresh）
    fresh_gen: Option<u64>,
    /// 这个角色的记录：(类, 容器序号) → 配方
    recs: HashMap<(usize, u32), Rec>,
    /// 容器第一次改之前，自己留的字段的样子
    keep: HashMap<(usize, u32), Vec<(usize, Vec<u8>)>>,
    pending: Option<Pending>,
    /// 还有没鉴定完的（上一批到了就接着发）
    more: bool,
    /// 定向鉴定还有没鉴定完的：(武器类型, 稀有度，5 = 全部)
    more_dir: Option<DirReq>,
    batches: u32,
    /// 开局装备：参数行改好了没有、试了几次（每 30 帧试一次）
    start_rows: bool,
    start_tries: u32,
    start_wait: u32,
    /// 许愿：还没许的次数（上一批还在发货时先攒着）、上一次找到的地区档
    wish_queue: u32,
    wish_band: Option<usize>,
    /// 开发工具（devgive.rs）：每 60 帧看一次 devgive.txt 的修改时间
    dev_wait: u32,
    dev_mtime: Option<SystemTime>,
    /// 回购商店：第 k 格 = 商店第 k 行（新卖的在前；买回去的那格先空着，下次有卖掉的再挤紧）、上次数到的背包 + 仓库、商店行要重写
    sold: Vec<Option<Rec>>,
    last_held: Option<HashSet<(usize, u32)>>,
    bb_dirty: bool,
    /// 回购这一步挤掉的（buyback_tick 另存到「挤掉的装备_<格子>.txt」后清空）
    evicted: Vec<Rec>,
    /// 别的模块要发的东西（gift）正在发：点亮发货标记的时间、这一批
    gift_wait: Option<GiftWait>,
    /// 进存档时已经强化到新上限（rmax）以上的武器容器序号：强化照原版（新的强化表没有那一级）
    over_max: HashSet<u32>,
    /// 护符模板换上稀有度边框了没有、试了几次（frame_templates）
    tal_frames: bool,
    tal_wait: u32,
    tal_tries: u32,
    /// 暗黑商店的武器商店：第 k 格 = items.wshop 第 k 行现在卖的（生成好、还没买的）；进存档时记录里上一批的（稳定后看哪些被买走了）；
    /// 这次进存档摆过货没有
    shop: Vec<Option<Rec>>,
    shop_prev: Vec<Rec>,
    shop_init: bool,
    /// 锁定了、出售价格改成 -1（卖出商店不显示）的物品：(类, 编号) → 原来的价格（解锁时改回）
    unsell: HashMap<(usize, u32), i32>,
    /// 词条数值随机：身上的随机词条挂的占位行（gear/valfx.rs）
    val: valfx::ValSt,
    /// 死亡惩罚：看血量认真死了没有、欠着没扣的、复活后要提示的（gear/deathloss.rs）
    death: deathloss::DeathSt,
}

/// 别的模块（唤声船的碎片 / 战灰）和装备回收要发的东西：（掉落表类别, 编号, 个数），一行一格；新装备系统没在发货时借发货表发
static GIFTS: Mutex<Vec<(i32, i32, u8)>> = Mutex::new(Vec::new());
/// 发货表一格最多放几个（掉落表的个数是一个字节）
const GIFT_MAX: u32 = 99;

/// 排队发一件东西（掉落表类别：1 道具、2 武器、3 防具、4 护符、5 战灰）。没有发货表（这次没用重随的模式启动）就返回 false
pub(crate) fn gift(cat: i32, id: i32) -> bool {
    gift_n(cat, id, 1)
}

/// 还在排队、没发出去的东西有几格（斗技场试炼收回奖励前要等发完，arena.rs）
pub(crate) fn gifts_pending() -> usize {
    GIFTS.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// 只进边上的获得记录、不弹屏幕中间的「获得物品」框（用户 2026-11-22「竞技场的奖励发放都走不弹窗」→「可以是不弹屏幕中间的框
/// 只弹边上的框吗」）：道具参数的 showDialogCondType（获得时弹框：0 不弹 / 2 第一次拿到才弹，原版道具全是 2）决定弹不弹框，
/// showLogCondType 决定进不进边上的记录（原版全是开着的）。照常走发货表，发这批之前把这些道具的弹框临时改成 0，到了以后
/// LOG_RESTORE_AFTER 再改回原来的值。还要这样发几件记在 LOG_ONLY（编号 → 件数），发货时按件数认（同一样东西别处照常发的照常弹框）
static LOG_ONLY: Mutex<Vec<(i32, u32)>> = Mutex::new(Vec::new());
/// 改过弹框的道具：(编号, 原来的值, 什么时候改回去；None = 那一批还没到)
static LOG_RESTORE: Mutex<Vec<(u32, u8, Option<Instant>)>> = Mutex::new(Vec::new());
/// 这批到了以后过多久改回弹框（游戏接着显示获得记录时可能还要读参数）
const LOG_RESTORE_AFTER: f32 = 3.0;

/// 直接放进背包、不弹框的（用户 2026-11-22「直接给+闪烁提示奖励已发放」→「只有无尽这么改」：无尽试炼每波的奖励，arena.rs）：
/// 道具编号 → 件数。gear/deathloss.rs 的 direct_drain 取走，排进死亡取回的 back，同一个公共事件（2003:43）发，全部发完提示 endless reward
static DIRECT: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

/// 排队直接给 n 个道具 goods（无尽试炼的奖励）。没有定义（不是用重随启动）就返回 false
pub(crate) fn give_direct(goods: u32, n: u32) -> bool {
    if defs().is_none_or(|d| d.items.deathback.is_none() && d.items.deliver_lot == 0) {
        return false;
    }
    if n > 0 {
        let mut q = DIRECT.lock().unwrap_or_else(|e| e.into_inner());
        match q.iter_mut().find(|x| x.0 == goods) {
            Some(x) => x.1 += n,
            None => q.push((goods, n)),
        }
    }
    true
}

/// 取走排着的直接给的
fn take_direct() -> Vec<(u32, u32)> {
    std::mem::take(&mut *DIRECT.lock().unwrap_or_else(|e| e.into_inner()))
}

/// 排队发 n 个道具 goods，只进边上的获得记录（斗技场的奖励，arena.rs）。没有发货表就返回 false
pub(crate) fn gift_log(goods: u32, n: u32) -> bool {
    if !gift_n(1, goods as i32, n) {
        return false;
    }
    let mut q = LOG_ONLY.lock().unwrap_or_else(|e| e.into_inner());
    add_count(&mut q, goods as i32, n);
    true
}

fn add_count(v: &mut Vec<(i32, u32)>, id: i32, n: u32) {
    if n == 0 {
        return;
    }
    match v.iter_mut().find(|x| x.0 == id) {
        Some(x) => x.1 += n,
        None => v.push((id, n)),
    }
}

/// 这一批里哪些要只进记录：按 LOG_ONLY 的件数认（道具类才算），认到的从 LOG_ONLY 里扣掉。返回 (编号, 这批里认了几件)
fn take_log_only(want: &mut Vec<(i32, u32)>, items: &[(i32, i32, u8)]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &(cat, id, n) in items {
        if cat != 1 {
            continue;
        }
        let Some(w) = want.iter_mut().find(|x| x.0 == id) else { continue };
        let k = w.1.min(n as u32);
        if k == 0 {
            continue;
        }
        w.1 -= k;
        match out.iter_mut().find(|x| x.0 == id as u32) {
            Some(x) => x.1 += k,
            None => out.push((id as u32, k)),
        }
    }
    want.retain(|x| x.1 > 0);
    out
}

/// 这批要只进记录的道具：弹框改成 0（原来的值记下，已经改着的不重记），等到了再改回去
fn log_dialog_off(ids: &[(u32, u32)]) {
    if ids.is_empty() {
        return;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let mut r = LOG_RESTORE.lock().unwrap_or_else(|e| e.into_inner());
    for &(id, _) in ids {
        let Some(g) = repo.get_mut::<EquipParamGoods>(id) else { continue };
        match r.iter_mut().find(|x| x.0 == id) {
            Some(x) => x.2 = None,
            None => r.push((id, g.show_dialog_cond_type(), None)),
        }
        g.set_show_dialog_cond_type(0);
    }
}

/// 这批到了（或者重新排队了）：几秒后把弹框改回去
fn log_dialog_due(ids: &[(u32, u32)]) {
    let due = Instant::now() + std::time::Duration::from_secs_f32(LOG_RESTORE_AFTER);
    let mut r = LOG_RESTORE.lock().unwrap_or_else(|e| e.into_inner());
    for x in r.iter_mut().filter(|x| ids.iter().any(|i| i.0 == x.0) && x.2.is_none()) {
        x.2 = Some(due);
    }
}

/// 到点的改回原来的弹框
fn log_dialog_restore() {
    let mut r = LOG_RESTORE.lock().unwrap_or_else(|e| e.into_inner());
    if r.is_empty() {
        return;
    }
    let now = Instant::now();
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    r.retain(|&(id, old, due)| {
        if due.is_none_or(|t| now < t) {
            return true;
        }
        if let Some(g) = repo.get_mut::<EquipParamGoods>(id) {
            g.set_show_dialog_cond_type(old);
        }
        false
    });
}

/// 重新排队的那一批：要只进记录的件数放回 LOG_ONLY（下一批照样不弹框）
fn log_only_again(ids: &[(u32, u32)]) {
    if ids.is_empty() {
        return;
    }
    let mut q = LOG_ONLY.lock().unwrap_or_else(|e| e.into_inner());
    for &(id, n) in ids {
        add_count(&mut q, id as i32, n);
    }
    drop(q);
    log_dialog_due(ids);
}

/// 发 n 个随机种类、指定稀有度（0 普通 … 3 传说）的骨灰容器（斗技场试炼的奖励，arena.rs；只进边上的获得记录）：返回每个的说明（日志用）和发出去的道具编号
pub(crate) fn give_ashes(rar: usize, n: u32) -> (Vec<String>, Vec<u32>) {
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else { return (vec!["还没进存档，不发".into()], Vec::new()) };
    let spec = ashes::AshSpec { rar: Some(rar.min(3)), log_only: true, ..Default::default() };
    ashes::dev_give_ids(&gdm.main_player_game_data, &spec, n)
}

/// 发 n 个随机种类的骨灰容器，稀有度、词条照拿到骨灰时的规则抽（死亡以上 Boss 掉的骨灰，ovldrop.rs）：返回每个的说明和发出去的道具编号
pub(crate) fn give_ashes_rolled(n: u32) -> (Vec<String>, Vec<u32>) {
    let Ok(gdm) = (unsafe { GameDataMan::instance() }) else { return (vec!["还没进存档，不发".into()], Vec::new()) };
    ashes::dev_give_ids(&gdm.main_player_game_data, &ashes::AshSpec::default(), n)
}

/// 排队发 n 个同样的东西（每格最多 GIFT_MAX 个）
pub(crate) fn gift_n(cat: i32, id: i32, n: u32) -> bool {
    if defs().is_none_or(|d| d.items.deliver_lot == 0) {
        return false;
    }
    let mut q = GIFTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut left = n;
    while left > 0 {
        let k = left.min(GIFT_MAX);
        q.push((cat, id, k as u8));
        left -= k;
    }
    true
}

/// 发出去等结果的一批
pub(crate) struct GiftWait {
    t0: Instant,
    items: Vec<(i32, i32, u8)>,
    /// 联机会话里：点亮标记前各样东西（掉落表类别, 编号）在背包 + 仓库里的个数；不在会话里 None（标记熄了就算到了）
    before: Option<Vec<((i32, i32), u32)>>,
    off_at: Option<Instant>,
    /// 这批里只进边上获得记录的道具（编号, 件数；gift_log）：发之前弹框改成 0 了，到了以后改回去
    log: Vec<(u32, u32)>,
}

/// 掉落表的类别（1 道具、2 武器、3 防具、4 护符）→ 背包里的类别
fn inv_cat_of_lot(cat: i32) -> Option<u8> {
    match cat {
        1 => Some(INV_GOODS),
        2..=4 => Some(INV_CAT[(cat - 2) as usize]),
        _ => None,
    }
}

/// 背包 + 仓库里这一样（掉落表的类别、编号）一共几个
/// 不用 items()：它只读「当前」的重要物品列表，联机时（Seamless 哪怕只有自己一人）换成联机用的那份，里面只有圣杯瓶、露滴的复制品，
/// 别的重要物品读不到（黄金树爬塔 native/src/loot.rs 记的；思路借鉴，代码自己写）。普通列表照常数，重要物品取两份里较多的
fn count_lot_item(pgd: &PlayerGameData, cat: i32, id: i32) -> u32 {
    let Some(ic) = inv_cat_of_lot(cat) else { return 0 };
    let hit = |it: &&eldenring::cs::EquipInventoryDataListEntry| it.item_id.category() as u8 == ic && it.item_id.param_id() as i64 == id as i64;
    let count = |inv: &EquipInventoryData| -> u32 {
        let d = &inv.items_data;
        let n = d.normal_entries().iter().filter_map(|e| e.as_option()).filter(hit).map(|it| it.quantity.max(1)).sum::<u32>();
        let k = d.key_entries().iter().filter_map(|e| e.as_option()).filter(hit).map(|it| it.quantity.max(1)).sum::<u32>();
        let m = d.multiplay_key_entries().iter().filter_map(|e| e.as_option()).filter(hit).map(|it| it.quantity.max(1)).sum::<u32>();
        n + k.max(m)
    };
    count(&pgd.equipment.equip_inventory_data) + pgd.storage.as_ref().map_or(0, |s| count(s))
}

/// 一批里每一样要涨多少
fn gift_need(items: &[(i32, i32, u8)]) -> Vec<((i32, i32), u32)> {
    let mut v: Vec<((i32, i32), u32)> = Vec::new();
    for &(c, i, n) in items {
        match v.iter_mut().find(|(k, _)| *k == (c, i)) {
            Some((_, x)) => *x += n as u32,
            None => v.push(((c, i), n as u32)),
        }
    }
    v
}

/// 发 gift 排的队：标记熄灭（公共事件发完）就算到了；15 秒还没熄就算没发（重新排回去，下次再发）。
/// 联机会话里多一步（联机计划 13.3 C8）：熄了再等 1 秒数背包 + 仓库，没涨够的重新排队
fn gift_tick(d: &Defs, st: &mut St, pgd: &PlayerGameData) {
    log_dialog_restore();
    if let Some(w) = st.gift_wait.as_mut() {
        if w.off_at.is_none() && flag_get(d.items.deliver_flag) == Some(false) {
            w.off_at = Some(Instant::now());
        }
        if let Some(off) = w.off_at {
            if w.before.is_some() && off.elapsed().as_secs_f32() < 1.0 {
                return;
            }
            let w = st.gift_wait.take().unwrap();
            let mut again: Vec<(i32, i32, u8)> = Vec::new();
            if let Some(before) = &w.before {
                for ((c, i), need) in gift_need(&w.items) {
                    let b = before.iter().find(|(k, _)| *k == (c, i)).map_or(0, |x| x.1);
                    let got = count_lot_item(pgd, c, i).saturating_sub(b);
                    let mut miss = need.saturating_sub(got);
                    while miss > 0 {
                        let k = miss.min(GIFT_MAX);
                        again.push((c, i, k as u8));
                        miss -= k;
                    }
                }
            }
            if again.is_empty() {
                log(&format!("发货：{} 格到了（{}）{}{}", w.items.len(), w.items.iter().map(|(c, i, n)| format!("{c}:{i}×{n}")).collect::<Vec<_>>().join("、"),
                             if w.before.is_some() { "，联机：数过背包了" } else { "" },
                             if w.log.is_empty() { String::new() } else { format!("；其中 {} 件只进获得记录、没弹框", w.log.iter().map(|x| x.1).sum::<u32>()) }));
                log_dialog_due(&w.log);
            } else {
                log(&format!("发货：联机时发货标记熄了，但背包里没涨够：{}，这几样重新排队",
                             again.iter().map(|(c, i, n)| format!("{c}:{i}×{n}")).collect::<Vec<_>>().join("、")));
                // 重新排的里面原来要只进记录的，下一批照样只进记录
                let mut was: Vec<(i32, u32)> = w.log.iter().map(|&(i, n)| (i as i32, n)).collect();
                log_only_again(&take_log_only(&mut was, &again));
                log_dialog_due(&w.log);
                GIFTS.lock().unwrap_or_else(|e| e.into_inner()).splice(0..0, again);
            }
            return;
        }
        if w.t0.elapsed().as_secs_f32() > 15.0 {
            log("发货：点亮发货标记 15 秒了还没熄，这批重新排队");
            let w = st.gift_wait.take().unwrap();
            let _ = lot_write(&d.items, &[]);
            flag_set(d.items.deliver_flag, false);
            log_only_again(&w.log);
            GIFTS.lock().unwrap_or_else(|e| e.into_inner()).splice(0..0, w.items);
        }
        return;
    }
    let items: Vec<(i32, i32, u8)> = {
        let mut q = GIFTS.lock().unwrap_or_else(|e| e.into_inner());
        let n = q.len().min(d.items.deliver_rows as usize);
        q.drain(..n).collect()
    };
    if items.is_empty() {
        return;
    }
    // 联机会话里点亮前先数一遍（C8）
    let before = crate::coop::active().then(|| gift_need(&items).into_iter().map(|((c, i), _)| ((c, i), count_lot_item(pgd, c, i))).collect());
    // 斗技场的奖励（gift_log）：这批里的这几样先关掉弹框，只进边上的获得记录
    let log_ids = take_log_only(&mut LOG_ONLY.lock().unwrap_or_else(|e| e.into_inner()), &items);
    log_dialog_off(&log_ids);
    match lot_write(&d.items, &items) {
        Ok(()) if flag_set(d.items.deliver_flag, true) => st.gift_wait = Some(GiftWait { t0: Instant::now(), items, before, off_at: None, log: log_ids }),
        r => {
            log(&format!("发货：改写发货表 / 点亮标记失败（{}），这批重新排队", r.err().unwrap_or_else(|| "标记".into())));
            log_only_again(&log_ids);
            GIFTS.lock().unwrap_or_else(|e| e.into_inner()).splice(0..0, items);
        }
    }
}

/// 这个角色的装备：记录里的，或者在回购商店里的
fn mine(st: &St, k: (usize, u32)) -> bool {
    st.recs.contains_key(&k) || st.sold.iter().flatten().any(|r| (r.cat, r.idx) == k)
}

static ST: Mutex<Option<St>> = Mutex::new(None);

/// 开局装备的配方（按 start 行的种子生成，每次启动一样）
static START: OnceLock<Vec<Rec>> = OnceLock::new();
const START_MAX_TRIES: u32 = 20;

fn start_recs(d: &Defs) -> &'static [Rec] {
    START.get_or_init(|| {
        let mut out = Vec::new();
        for s in &d.items.start {
            let mut rng = Rng(s.seed);
            let recipe = match s.cat {
                0 => gen_weapon_on(&d.lib, s.tier as usize, Some(s.wfix()), &mut rng),
                _ => gen_armor_on(&d.lib, s.tier as usize, Some(s.base), s.full, &mut rng),
            };
            match (recipe, start_off(&d.items, s.cat, s.id)) {
                (Some(recipe), Some(idx)) => out.push(Rec { cat: s.cat, idx, tier: s.tier, recipe }),
                (None, _) => log(&format!("新装备系统：开局装备 {} 的本体 {} 不在词条库里，没生成", s.id, s.base)),
                (_, None) => log(&format!("新装备系统：开局装备 {} 不在开局装备的编号段里（items.txt 没有 startids？），没生成", s.id)),
            }
        }
        out
    })
}

/// 开局装备：一启动（参数载入后，还在主菜单）就改写参数行，扫内存写文字（进了存档由 texts_tick 一起写）
fn start_tick(d: &'static Defs, st: &mut St) {
    let recs = start_recs(d);
    if recs.is_empty() || st.start_rows {
        return;
    }
    st.start_wait = st.start_wait.wrapping_add(1);
    if st.start_wait % 30 != 1 {
        return;
    }
    st.start_tries += 1;
    let mut errs = Vec::new();
    for r in recs {
        if st.who.is_some() && mine(st, (r.cat, r.idx)) {
            continue; // 已经在存档里：这个角色记录里的那份由 on_load 改好了
        }
        if let Err(e) = write_row(d, st, r) {
            errs.push(e);
        }
    }
    if !errs.is_empty() && st.start_tries < START_MAX_TRIES {
        return;
    }
    st.start_rows = true;
    log(&format!(
        "新装备系统：开局装备按种子生成、改写参数行 {} 件{}",
        recs.len() - errs.len(),
        if errs.is_empty() { String::new() } else { format!("（{} 件失败：{}）", errs.len(), errs[0]) }
    ));
    texts_fresh();
}

/// 还在主菜单（没进存档）时，把开局装备的文字交上（建新角色时看到的就是这一份）
fn start_texts(d: &Defs, st: &St) {
    if st.who.is_some() || !st.start_rows {
        return;
    }
    let mut g = TXT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(t) = g.as_mut() else { return };
    if !t.fresh {
        return;
    }
    t.fresh = false;
    let n: usize = start_recs(d).iter().map(|r| write_texts(d, r)).sum();
    log(&format!("新装备系统：开局装备的文字交好了 {n} 条{}", if crate::msghook::hooked() { "" } else { "（取文字挂钩还没挂上）" }));
}

/// 角色记录的文件夹 %APPDATA%\EldenRing\DarkReforged\角色记录（和存档、启动前备份放一起，整合包换目录 / 更新都不丢）；没有 APPDATA 就用 DLL 所在目录。
/// 联机（Seamless Co-op 加载了，用联机存档）时是里面的 联机\：联机存档的格子编号和单机各算各的，不分开会读写同一份记录（coop.rs 的 ersc_loaded）
fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("EldenRing").join("DarkReforged").join("角色记录")).unwrap_or_else(dir);
    if crate::coop::ersc_loaded() { base.join("联机") } else { base }
}

/// 读一个角色记录：新位置没有就读旧版放在 %APPDATA%\EldenRing 根目录的 DarkReforged_<名字>（2026-10-29 以前；启动器会搬过来，这里兜底）。
/// 联机时不兜底（旧位置的是单机角色的）。写总是写新位置
fn rec_read(name: &str) -> std::io::Result<String> {
    fs::read_to_string(data_dir().join(name)).or_else(|e| match std::env::var_os("APPDATA") {
        Some(a) if !crate::coop::ersc_loaded() => fs::read_to_string(PathBuf::from(a).join("EldenRing").join(format!("DarkReforged_{name}"))),
        _ => Err(e),
    })
}

fn rec_path(slot: i32) -> PathBuf {
    data_dir().join(format!("{REC_PREFIX}{slot}.txt"))
}

/// 角色记录里放「挪开的旧记录」的文件夹：同一格子换了角色（文件里的角色名和现在的对不上）时，原来那份先复制到这里再写新的，
/// 读档时名字对不上就到这里找回这个角色的（玩家诊断包 2026-11-18：新建角色那一刻游戏报的格子还是上一个角色的，
/// 上一个角色的记录被新角色的开局装备直接盖掉了，220 件装备变成占位文字；用户「改」）。装备、骨灰的记录都用
const OLD_DIR: &str = "旧记录";

/// 记录文本里的角色名（跳过 # 开头的说明行，第一行「name<TAB>角色名」）；没有就 None
pub(super) fn rec_text_name(text: &str) -> Option<String> {
    let l = text.lines().map(|l| l.trim_start_matches('\u{feff}')).find(|l| !l.starts_with('#'))?;
    l.trim_end_matches('\r').strip_prefix("name\t").map(str::to_string)
}

/// 要写 path 了：它原来是别的角色的记录（名字不是 name、也不是空的）就先复制到 旧记录\<文件名>_<时间>.txt。返回复制了没有
pub(super) fn rec_set_aside(path: &std::path::Path, name: &str) -> bool {
    let Some(old) = fs::read_to_string(path).ok().as_deref().and_then(rec_text_name) else { return false };
    if old.is_empty() || old == name {
        return false;
    }
    let dst = rec_aside_copy(path);
    log(&format!(
        "角色记录：{} 原来是「{old}」的，现在要写「{name}」的；原来那份{}",
        path.display(),
        dst.as_ref().map_or("另存失败（照样写新的）".to_string(), |p| format!("另存到 {}", p.display()))
    ));
    dst.is_some()
}

/// 把 path 复制到 旧记录\<文件名>_<时间>.txt（不管是谁的；刚建的角色挪开同名旧角色的记录也用它）；没有这个文件或复制失败返回 None
pub(super) fn rec_aside_copy(path: &std::path::Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    let dir = path.parent()?.join(OLD_DIR);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("record");
    let t: String = crate::now_text().chars().filter(|c| c.is_ascii_digit()).collect();
    let mut dst = dir.join(format!("{stem}_{t}.txt"));
    let mut k = 1;
    while dst.exists() {
        k += 1;
        dst = dir.join(format!("{stem}_{t}_{k}.txt"));
    }
    fs::create_dir_all(&dir).and_then(|_| fs::copy(path, &dst)).ok().map(|_| dst)
}

/// 刚建的角色：这个格子以前的锁定（锁定名单只按格子记）清掉。dark_lock 自己内存里的那份经 dr_who_fresh 知道、也会清
fn lock_clear_slot(slot: i32) {
    let p = data_dir().join(LOCK_FILE);
    let Ok(text) = fs::read_to_string(&p) else { return };
    let mut gone = 0;
    let keep: Vec<&str> = text
        .lines()
        .filter(|l| {
            let mine = !l.starts_with('#') && l.split('\t').next().and_then(|s| s.trim_start_matches('\u{feff}').parse::<i32>().ok()) == Some(slot);
            gone += mine as usize;
            !mine
        })
        .collect();
    if gone == 0 {
        return;
    }
    let ok = fs::write(&p, keep.join("\n") + "\n").is_ok();
    log(&format!("锁定名单：格子 {slot} 是刚建的角色，清掉这个格子以前的 {gone} 条锁定{}", if ok { "" } else { "（写不进去）" }));
}

/// path 里不是 name 的记录：到 旧记录 里找同一个文件名开头（同一格子）、角色名是 name 的，最新的一份 → (路径, 内容)
pub(super) fn rec_find_aside(path: &std::path::Path, name: &str) -> Option<(PathBuf, String)> {
    if name.is_empty() {
        return None;
    }
    let dir = path.parent()?.join(OLD_DIR);
    let prefix = format!("{}_", path.file_stem()?.to_str()?);
    let mut best: Option<(SystemTime, PathBuf, String)> = None;
    for e in fs::read_dir(&dir).ok()?.flatten() {
        let p = e.path();
        let Some(f) = p.file_name().and_then(|f| f.to_str()) else { continue };
        if !f.starts_with(&prefix) || !f.ends_with(".txt") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&p) else { continue };
        if rec_text_name(&text).as_deref() != Some(name) {
            continue;
        }
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        if best.as_ref().is_none_or(|b| t > b.0) {
            best = Some((t, p, text));
        }
    }
    best.map(|(_, p, text)| (p, text))
}

/// 读出来的记录：这个角色的、回购商店里的、武器商店正在卖的
type Loaded = (HashMap<(usize, u32), Rec>, Vec<Rec>, Vec<Rec>);

/// 记录文件：第一行「name<TAB>角色名」，后面一行一个容器；「sold<TAB>」开头的是回购商店里的（按格子顺序），
/// 「shop<TAB>」开头的是武器商店正在卖的（买了还没记进来就退出的话，下次进存档按这个补记）。
/// 角色名对不上（这个格子换了新角色）就当没有
fn rec_load(text: &str, name: &str) -> Option<Loaded> {
    let mut it = text.lines().filter(|l| !l.starts_with('#'));
    let head = it.next()?;
    if head.trim_end_matches('\r') != format!("name\t{name}") {
        return None;
    }
    let (mut recs, mut sold, mut shop) = (HashMap::new(), Vec::new(), Vec::new());
    for l in it {
        if let Some(s) = l.strip_prefix("sold\t") {
            sold.extend(rec_parse(s));
        } else if let Some(s) = l.strip_prefix("shop\t") {
            shop.extend(rec_parse(s));
        } else if let Some(r) = rec_parse(l) {
            recs.insert((r.cat, r.idx), r);
        }
    }
    Some((recs, sold, shop))
}

fn rec_text(name: &str, recs: &HashMap<(usize, u32), Rec>, sold: &[Option<Rec>], shop: &[Rec]) -> String {
    let mut v: Vec<&Rec> = recs.values().collect();
    v.sort_by_key(|r| (r.cat, r.idx));
    let mut out = String::from(
        "# 新装备系统（dll\\dark_core）：这个存档格子的角色鉴定出来的装备，一行一个容器的配方；sold 开头的是回购商店里的（最后卖掉的，新的在前）。\
         删掉的话这些装备会变回没改写的样子\n",
    );
    out.push_str(&format!("name\t{name}\n"));
    for r in v {
        out.push_str(&rec_line(r));
        out.push('\n');
    }
    for r in sold.iter().flatten() {
        out.push_str(&format!("sold\t{}\n", rec_line(r)));
    }
    for r in shop {
        out.push_str(&format!("shop\t{}\n", rec_line(r)));
    }
    out
}

/// 记录要等身份认准（who.rs：人物进到世界里、「格子 + 名字」稳定约 1 秒）才写（用户 2026-11-19「改」「收拢」：
/// 建新角色时游戏报的格子号还是上一个角色的、名字已经是新角色的，以前进存档那一刻就把新角色的开局装备写进了上一个角色的格子；
/// 三份诊断包里这种临时的格子号都只出现在「人物还没生成」时，真正的格子号在人物出现之前就换好了）。在那之前要写的先欠着，
/// 认准后补写（tick 里）；换了角色就作废（那份是上一个身份的）。旧记录（rec_set_aside）留作第二道保险
static REC_OWED: AtomicBool = AtomicBool::new(false);

fn rec_save(who: &(i32, String), st: &St) {
    if who.1.is_empty() {
        log("新装备系统：角色名是空的，不写记录（免得把这个格子的记录覆盖掉）");
        return;
    }
    if !crate::who::is_confirmed(who) {
        if !REC_OWED.swap(true, Ordering::Relaxed) {
            log(&format!("新装备系统：格子 {}「{}」还没进到世界里，记录先不写，人物出现、稳定后再写", who.0, who.1));
        }
        return;
    }
    let p = rec_path(who.0);
    let _ = fs::create_dir_all(data_dir());
    rec_set_aside(&p, &who.1); // 这一格原来是别的角色的记录：先另存，不直接盖掉
    let tmp = p.with_extension("txt.tmp");
    let shop: Vec<Rec> = st.shop.iter().flatten().chain(&st.shop_prev).cloned().collect();
    if fs::write(&tmp, rec_text(&who.1, &st.recs, &st.sold, &shop)).and_then(|_| fs::rename(&tmp, &p)).is_err() {
        log(&format!("新装备系统：写记录 {} 失败", p.display()));
    }
}

// ------------------------------------------------------------------ 游戏里的东西
pub(crate) fn flag_get(f: u32) -> Option<bool> {
    unsafe { CSEventFlagMan::instance_mut() }.ok().map(|m| m.virtual_memory_flag.get_flag(f))
}

/// dark_core 写标记都走这里：存档标记护栏（flagguard.rs）据此认出「本机写的」
pub(crate) fn flag_set(f: u32, on: bool) -> bool {
    crate::flagguard::own(f, on);
    flag_set_raw(f, on)
}

/// 直接写，不告诉护栏（护栏改回外来改动时用）
pub(crate) fn flag_set_raw(f: u32, on: bool) -> bool {
    unsafe { CSEventFlagMan::instance_mut() }.ok().map(|m| m.virtual_memory_flag.set_flag(f, on)).is_some()
}

fn player_anim() -> Option<i32> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let p = wcm.main_player.as_ref()?;
    let ta = &p.chr_ins.modules.time_act;
    Some(ta.anim_queue[ta.read_idx as usize % ta.anim_queue.len()].anim_id)
}

/// 坐在赐福点（同 dark_sell）
fn at_grace() -> bool {
    matches!(player_anim(), Some(68000..=68099))
}

/// 背包 + 仓库里的容器（类, 序号）
fn held(d: &Defs, pgd: &PlayerGameData) -> HashSet<(usize, u32)> {
    let mut out = HashSet::new();
    let mut add = |inv: &EquipInventoryData| {
        for it in inv.items_data.items() {
            let cat = it.item_id.category() as u8;
            if let Some(c) = INV_CAT.iter().position(|&x| x == cat) {
                let pid = it.item_id.param_id();
                let pid = if c == 0 { pid / 10000 * 10000 } else { pid };
                if let Some(k) = d.items.cont[c].idx_of(pid) {
                    out.insert((c, k));
                } else if let Some(k) = start_off(&d.items, c, pid) {
                    out.insert((c, k));
                }
            }
        }
    };
    add(&pgd.equipment.equip_inventory_data);
    if let Some(s) = &pgd.storage {
        add(&**s);
    }
    out
}

/// 背包 + 仓库里强化等级超过 max 的武器容器序号（物品编号末两位是强化等级）
fn over_max(d: &Defs, pgd: &PlayerGameData, max: u32) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut add = |inv: &EquipInventoryData| {
        for it in inv.items_data.items() {
            if it.item_id.category() as u8 != INV_CAT[0] {
                continue;
            }
            let pid = it.item_id.param_id();
            if pid % 100 <= max {
                continue;
            }
            let base = pid / 10000 * 10000;
            if let Some(k) = d.items.cont[0].idx_of(base).or_else(|| start_off(&d.items, 0, base)) {
                out.insert(k);
            }
        }
    };
    add(&pgd.equipment.equip_inventory_data);
    if let Some(s) = &pgd.storage {
        add(&**s);
    }
    out
}

/// 背包里的未鉴定道具：(道具编号, 件数)
fn unidentified(d: &Defs, pgd: &PlayerGameData) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = pgd
        .equipment
        .equip_inventory_data
        .items_data
        .items()
        .filter(|it| it.item_id.category() as u8 == INV_GOODS && d.items.goods.contains_key(&it.item_id.param_id()) && it.quantity > 0)
        .map(|it| (it.item_id.param_id(), it.quantity))
        .collect();
    v.sort();
    v
}

/// 这个角色每类还能用的容器：不在记录里，或者在记录里但已经不在背包 / 仓库里（卖了、丢了）；回购商店、武器商店里的不用；
/// 这个角色锁定名单里有的不用（dark_lock 只在内存里管名单，这里不去改它的文件，免得旧的锁定跟到新装备上）
fn free(d: &Defs, st: &St, held: &HashSet<(usize, u32)>) -> [Vec<u32>; NCAT] {
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    let sold: HashSet<(usize, u32)> = st.sold.iter().flatten().map(|r| (r.cat, r.idx)).collect();
    let shop: HashSet<(usize, u32)> = st.shop.iter().flatten().chain(&st.shop_prev).map(|r| (r.cat, r.idx)).collect();
    let mut out: [Vec<u32>; NCAT] = Default::default();
    for (c, v) in out.iter_mut().enumerate() {
        let ct = &d.items.cont[c];
        *v = (0..ct.count)
            .filter(|k| {
                // 背包 / 仓库里有这一格的东西就不算空格，记录里有没有都一样（联机计划 13.7：联机时捡到队友丢的新装备，记录里没有；
                // 以前当空格，下次鉴定用上它，捡到的那件就跟着变成新鉴定的那件，背包里两件一样的）
                !held.contains(&(c, *k))
                    && !sold.contains(&(c, *k))
                    && !shop.contains(&(c, *k))
                    && !locked.contains(&(c, ct.id(*k)))
            })
            .collect();
    }
    out
}

const LOCK_FILE: &str = "lock_items.txt";

/// dark_lock 的锁定名单里这个角色的（类, 编号）：一行「格子<TAB>类别<TAB>编号（武器不含强化等级）<TAB>角色名」
fn locked_keys(text: &str, who: &(i32, String)) -> HashSet<(usize, u32)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            (f.len() >= 4 && f[0].parse::<i32>().ok() == Some(who.0) && f[3] == who.1)
                .then(|| Some((f[1].parse::<usize>().ok()?, f[2].parse::<u32>().ok()?)))
                .flatten()
        })
        .collect()
}

// ------------------------------------------------------------------ 弃置名单（装备工坊「超限」页的「弃置」，用户 2026-11-26）
/// 弃置名单：和锁定名单同目录同格式，一行「格子<TAB>0<TAB>编号<TAB>角色名」，只给生成武器用。
/// 编号是 base（param_id / 10000 * 10000）：每把生成武器独占一条参数行，所以精确到单把，强化后也不变
/// （强化等级是 pid - base，+1 步进）。锁定名单那边按「去掉末两位」记，对生成武器算出来同样是 base，这里直接用 base 更明确。
/// 写入：dark_core（工坊的弃置按钮、回收后删、修剪）、dark_lock（在背包里锁定时删一条）；
/// 读取：dark_core（快照、回收）、dark_sell（出售）。dark_sell 只读不写，避免两个 DLL 同时写同一份文件。
const DISCARD_FILE: &str = "discard_items.txt";

/// 名单全文（没有这个文件就是空的）
fn discard_text() -> String {
    fs::read_to_string(data_dir().join(DISCARD_FILE)).unwrap_or_default()
}

/// 名单里这个角色的 base 编号
fn discard_keys(text: &str, who: &(i32, String)) -> HashSet<u32> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            (f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0)).then(|| f[2].parse::<u32>().ok()).flatten()
        })
        .collect()
}

/// 写名单（先写 .tmp 再换名：写到一半失败不会留下半个文件）；首行固定是说明
fn discard_save(keep: &[String]) {
    let p = data_dir().join(DISCARD_FILE);
    let _ = fs::create_dir_all(data_dir());
    let mut out = String::from("# 格子\t类别（0 武器）\t编号（生成武器的 base = 参数行 id / 10000 * 10000）\t角色名\n");
    for l in keep {
        out.push_str(l);
        out.push('\n');
    }
    let tmp = p.with_extension("txt.tmp");
    if fs::write(&tmp, out).and_then(|_| fs::rename(&tmp, &p)).is_err() {
        log(&format!("弃置名单写不进 {}", p.display()));
    }
}

/// 把这把武器标成弃置 / 取消弃置，返回改完以后是不是弃置。读-改-写、不做内存缓存
/// （dark_lock 也可能改这份文件，有缓存就会把对方的改动写回去）
fn discard_toggle(who: &(i32, String), base: u32) -> bool {
    let mut keep: Vec<String> = Vec::new();
    let mut had = false;
    for l in discard_text().lines().map(|l| l.trim_end_matches('\r').to_string()) {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0) && f[2].parse::<u32>().ok() == Some(base) {
            had = true;
            continue;
        }
        keep.push(l);
    }
    if !had {
        keep.push(format!("{}\t0\t{base}\t{}", who.0, who.1));
    }
    discard_save(&keep);
    !had
}

/// 从这个角色的弃置名单里删掉这些 base（回收 / 出售之后用）
fn discard_remove(who: &(i32, String), bases: &[u32]) {
    if bases.is_empty() {
        return;
    }
    let (mut keep, mut hit) = (Vec::new(), 0);
    for l in discard_text().lines().map(|l| l.trim_end_matches('\r').to_string()) {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        let ours = f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0);
        if ours && f[2].parse::<u32>().ok().is_some_and(|b| bases.contains(&b)) {
            hit += 1;
            continue;
        }
        keep.push(l);
    }
    if hit > 0 {
        discard_save(&keep);
    }
}

/// 把一份背包 / 木箱里的生成武器 base 收进 out（弃置名单的修剪判据用）
fn weapon_bases(inv: &EquipInventoryData, out: &mut HashSet<u32>) {
    for e in inv.items_data.normal_entries() {
        let Some(it) = e.as_option() else { continue };
        if it.item_id.category() as u8 == INV_CAT[0] && it.quantity > 0 {
            out.insert(it.item_id.param_id() / 10000 * 10000);
        }
    }
}

/// 修剪：这个角色在名单里、但背包和木箱里都没有的 base 删掉（卖掉 / 丢掉 / 回收之后的幽灵条目），返回删了几条。
/// 必须同时看木箱：玩家把弃置的武器放进木箱后背包里就没了，只看背包会把标记误删，取回来时状态就丢了
fn discard_prune(pgd: &PlayerGameData, who: &(i32, String)) -> usize {
    let text = discard_text();
    if text.trim().is_empty() {
        return 0;
    }
    let mut live: HashSet<u32> = HashSet::new();
    weapon_bases(&pgd.equipment.equip_inventory_data, &mut live);
    if let Some(s) = &pgd.storage {
        weapon_bases(s, &mut live);
    }
    let (mut keep, mut gone) = (Vec::new(), 0);
    for l in text.lines().map(|l| l.trim_end_matches('\r').to_string()) {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        let ours = f.len() >= 3 && f[0].parse::<i32>().ok() == Some(who.0);
        if ours && f[2].parse::<u32>().ok().is_some_and(|b| !live.contains(&b)) {
            gone += 1;
            continue;
        }
        keep.push(l);
    }
    if gone > 0 {
        discard_save(&keep);
    }
    gone
}

/// 弃置 / 取消弃置一把生成武器（装备工坊「超限」页武器栏的第 4 个按钮，用户 2026-11-26）：
/// 只写名单、不动游戏数据（规则上只影响一键出售和武器回收）。装备着的、锁定的不能弃置
fn discard_apply(d: &Defs, st: &St, pgd: &PlayerGameData, who: &(i32, String), k: u32) -> Result<[String; 2], [String; 2]> {
    let e = |zh: &str, en: &str| [zh.to_string(), en.to_string()];
    let Some(rec) = st.recs.get(&(0, k)) else {
        return Err(e("这不是你的生成武器", "this is not your generated armament"));
    };
    let base = rec_id(&d.items, rec);
    let (eq_ids, _) = equipped(pgd);
    let inv = &pgd.equipment.equip_inventory_data;
    let mut found = false;
    for ent in inv.items_data.normal_entries() {
        let Some(it) = ent.as_option() else { continue };
        if it.item_id.category() as u8 != INV_CAT[0] || it.quantity == 0 || it.item_id.param_id() / 10000 * 10000 != base {
            continue;
        }
        if eq_ids.contains(&it.item_id.into_inner()) {
            return Err(e("装备着的武器不能弃置，先换下来", "an equipped armament can't be discarded; unequip it first"));
        }
        found = true;
        break;
    }
    if !found {
        return Err(e("这把武器不在背包里了", "the armament is no longer in your inventory"));
    }
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    if locked.contains(&(0, base)) {
        return Err(e("这把武器锁着，先解锁", "this armament is locked; unlock it first"));
    }
    let now = discard_toggle(who, base);
    let names = [plain_name(d, rec, 0), plain_name(d, rec, 1)];
    log(&format!("弃置：武器 {k}（{}）{}（{base}）", names[0], if now { "已弃置" } else { "已取消弃置" }));
    Ok(if now {
        [format!("「{}」已弃置", names[0]), format!("\"{}\" marked as discarded.", names[1])]
    } else {
        [format!("「{}」已取消弃置", names[0]), format!("\"{}\" is no longer marked as discarded.", names[1])]
    })
}

/// 锁定名单变了以后要做的：(改回原价的 [(物品, 原价)], 改成 -1 的 [(物品, 现在的价)])。
/// applied = 已经改成 -1 的（和原价）；want = 现在锁定的；cur = 参数行现在的出售价格（没有这一行就 None）。
/// 锁定的物品价格不是 -1 就改（第一次锁、或者参数行被重建成按稀有度的价格了），本来就是 -1 的不动
fn unsell_step(
    applied: &HashMap<(usize, u32), i32>,
    want: &HashSet<(usize, u32)>,
    cur: impl Fn((usize, u32)) -> Option<i32>,
) -> (Vec<((usize, u32), i32)>, Vec<((usize, u32), i32)>) {
    let mut restore: Vec<_> = applied.iter().filter(|(k, _)| !want.contains(k)).map(|(&k, &v)| (k, v)).collect();
    let mut hide: Vec<_> = want.iter().filter_map(|&k| cur(k).filter(|&v| v >= 0).map(|v| (k, v))).collect();
    restore.sort();
    hide.sort();
    (restore, hide)
}

/// 这件物品参数行的出售价格（锁定名单的类：0 武器 / 1 防具 / 2 护符）
fn sell_get(repo: &SoloParamRepository, (cat, id): (usize, u32)) -> Option<i32> {
    match cat {
        0 => repo.get::<EquipParamWeapon>(id).map(|p| p.sell_value()),
        1 => repo.get::<EquipParamProtector>(id).map(|p| p.sell_value()),
        2 => repo.get::<EquipParamAccessory>(id).map(|p| p.sell_value()),
        _ => None,
    }
}

fn sell_set(repo: &mut SoloParamRepository, (cat, id): (usize, u32), v: i32) {
    match cat {
        0 => repo.get_mut::<EquipParamWeapon>(id).map(|p| p.set_sell_value(v)),
        1 => repo.get_mut::<EquipParamProtector>(id).map(|p| p.set_sell_value(v)),
        2 => repo.get_mut::<EquipParamAccessory>(id).map(|p| p.set_sell_value(v)),
        _ => None,
    };
}

/// 锁定的物品不在卖出商店显示（用户 2026-10-30）：参数行的出售价格改成 -1（原版圣杯瓶、关键道具就是这样，卖出界面里不显示），
/// 解锁了改回原来的。每秒看一次锁定名单（dark_lock 写的 lock_items.txt 里这个角色的）；生成装备的行重建时出售价格会按稀有度写回，
/// 下一次看时再改成 -1。原版物品是同一个编号共用一行，同种的几件一起不显示（和锁定的规则一样）。who = None：全部改回（换角色）
fn unsell_tick(st: &mut St, who: Option<&(i32, String)>) {
    let want = who.map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    if want.is_empty() && st.unsell.is_empty() {
        return;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let (restore, hide) = unsell_step(&st.unsell, &want, |k| sell_get(repo, k));
    let fresh = hide.iter().filter(|(k, _)| !st.unsell.contains_key(k)).count();
    for (k, v) in &restore {
        sell_set(repo, *k, *v);
        st.unsell.remove(k);
    }
    for (k, v) in &hide {
        sell_set(repo, *k, -1);
        st.unsell.insert(*k, *v);
    }
    if !restore.is_empty() || fresh > 0 {
        log(&format!("锁定物品：{fresh} 件不在卖出商店显示（出售价格改成 -1），{} 件解锁改回原价", restore.len()));
    }
}

// 发货表
fn lot_fill(b: &mut [u8], off: [usize; 5], slot: Option<(i32, i32, u8)>) {
    let [o_id, o_cat, o_pt, o_num, o_flag] = off;
    for i in 0..8 {
        b[o_id + 4 * i..o_id + 4 * i + 4].fill(0);
        b[o_cat + 4 * i..o_cat + 4 * i + 4].fill(0);
        b[o_pt + 2 * i..o_pt + 2 * i + 2].fill(0);
        b[o_num + i] = 0;
    }
    let (cat, item, num) = slot.unwrap_or((0, 0, 0));
    b[o_id..o_id + 4].copy_from_slice(&item.to_le_bytes());
    b[o_cat..o_cat + 4].copy_from_slice(&cat.to_le_bytes());
    b[o_pt..o_pt + 2].copy_from_slice(&1000u16.to_le_bytes());
    b[o_num] = num;
    b[o_flag..o_flag + 4].fill(0);
}

/// 改写发货表：rows = （掉落表类别, 编号, 个数），一行一格，剩下的行清空
fn lot_write(it: &Items, rows: &[(i32, i32, u8)]) -> Result<(), String> {
    let n = std::mem::size_of::<<ItemLotParam_map as SoloParam>::StructType>();
    if n != it.lot_size {
        return Err(format!("掉落表一行的大小对不上（游戏 {n}，定义 {}）", it.lot_size));
    }
    if it.lot_off.iter().zip([32usize, 32, 16, 8, 4]).any(|(&o, l)| o + l > n) {
        return Err("掉落表的字段偏移超出一行".into());
    }
    let repo = unsafe { SoloParamRepository::instance_mut() }.map_err(|_| "参数还没载入".to_string())?;
    for r in 0..it.deliver_rows {
        let id = it.deliver_lot + r;
        let b = param_mut::<ItemLotParam_map>(repo, id).ok_or_else(|| format!("发货表 {id} 不在游戏参数里（这次不是用重随的模式启动的？）"))?;
        lot_fill(b, it.lot_off, rows.get(r as usize).copied());
    }
    Ok(())
}

// 移除物品（游戏自己的函数，特征码和 dark_sell 一样）：(背包, 格序号, 开关)，整格删掉；第三个参数只看是不是 0，不是数量（反汇编 exe+0x24d1f0）
const REMOVE_ITEM_AOB: [Option<u8>; 13] = [
    None, Some(0x83), Some(0xEC), None, Some(0x8B), Some(0xF2), None, Some(0x8B), Some(0xE9), None, Some(0x85), Some(0xC0), Some(0x74),
];
const REMOVE_ITEM_BACK: usize = 16;
type RemoveItemFn = unsafe extern "C" fn(*mut EquipInventoryData, u32, u32) -> u64;
static REMOVE_ITEM: OnceLock<Option<usize>> = OnceLock::new();

pub(crate) fn aob_scan_unique(pat: &[Option<u8>]) -> Result<usize, String> {
    let base = unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandle：{e}"))?.0 as usize;
    unsafe {
        let pe = base + *((base + 0x3C) as *const u32) as usize;
        let nsec = *((pe + 6) as *const u16) as usize;
        let opt_size = *((pe + 20) as *const u16) as usize;
        let sec0 = pe + 24 + opt_size;
        let mut hits = Vec::new();
        for s in 0..nsec {
            let sh = sec0 + 40 * s;
            if *((sh + 36) as *const u32) & 0x2000_0000 == 0 {
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
                    return Err("命中不止一处".into());
                }
            }
        }
        hits.first().copied().ok_or_else(|| "没找到".into())
    }
}

fn remove_item_fn() -> Option<usize> {
    *REMOVE_ITEM.get_or_init(|| match aob_scan_unique(&REMOVE_ITEM_AOB) {
        Ok(hit) => Some(hit - REMOVE_ITEM_BACK),
        Err(e) => {
            log(&format!("新装备系统：移除物品函数特征码 {e}，鉴定后扣不掉未鉴定道具"));
            None
        }
    })
}

/// 背包结构（和 PlayerGameData+0x5D0 读出的地址核对，同 dark_sell）
fn inventory(pgd: &mut PlayerGameData) -> Option<*mut EquipInventoryData> {
    let inv_ptr = &mut pgd.equipment.equip_inventory_data as *mut EquipInventoryData;
    let tga_ptr = unsafe { *((pgd as *const PlayerGameData as *const u8).add(0x5D0) as *const usize) };
    (tga_ptr == inv_ptr as usize).then_some(inv_ptr)
}

/// 从背包扣掉 n 个道具 goods，返回实际扣掉几个
pub(crate) fn remove_goods(pgd: &mut PlayerGameData, goods: u32, n: u32) -> u32 {
    let Some(f) = remove_item_fn() else { return 0 };
    let Some(inv_ptr) = inventory(pgd) else {
        log("新装备系统：背包结构地址对不上，扣不掉未鉴定道具");
        return 0;
    };
    let inv = unsafe { &mut *inv_ptr };
    let key_cap = inv.items_data.key_items_capacity;
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    let mut left = n;
    let entries = inv.items_data.normal_entries().len();
    for idx in 0..entries {
        if left == 0 {
            break;
        }
        let Some((raw, qty)) = inv.items_data.normal_entries()[idx]
            .as_option()
            .filter(|it| it.item_id.category() as u8 == INV_GOODS && it.item_id.param_id() == goods)
            .map(|it| (it.item_id.into_inner(), it.quantity))
        else {
            continue;
        };
        let take = qty.min(left);
        if take < qty {
            // 只扣一部分：游戏的移除函数不管第三个参数是几，都整格删掉（一次重铸扣光重铸石），所以直接改数量
            if let Some(it) = inv.items_data.normal_entries_mut()[idx].as_option_mut().filter(|it| it.item_id.into_inner() == raw) {
                it.quantity = qty - take;
                left -= take;
            }
            continue;
        }
        unsafe { remove(inv_ptr, key_cap + idx as u32, take) };
        let after = inv.items_data.normal_entries()[idx].as_option().filter(|it| it.item_id.into_inner() == raw).map(|it| it.quantity).unwrap_or(0);
        left -= qty.saturating_sub(after).min(take);
    }
    n - left
}

// ------------------------------------------------------------------ 每帧
/// 进存档：按记录改写所有容器的行；发货表清空、标记熄灭；文字等扫完内存再写
fn on_load(d: &'static Defs, st: &mut St, who: &(i32, String), pgd: &PlayerGameData) {
    unsell_tick(st, None); // 上一个角色锁定的改回能卖，这个角色的稳定后再按它的名单改
    let text = rec_read(&format!("{REC_PREFIX}{}.txt", who.0)).unwrap_or_default();
    let mut loaded = rec_load(&text, &who.1);
    let mut from = rec_path(who.0).display().to_string();
    if loaded.is_none() {
        // 这一格的记录是别的角色的（或者没有）：到 旧记录 里找这个角色以前被挪开的那份
        if let Some((p, t)) = rec_find_aside(&rec_path(who.0), &who.1) {
            loaded = rec_load(&t, &who.1);
            from = p.display().to_string();
            if let Some(l) = &loaded {
                log(&format!("新装备系统：格子 {} 的记录不是「{}」的，从 {} 找回 {} 件（回购 {} 件）", who.0, who.1, p.display(), l.0.len(), l.1.len()));
            }
        }
    }
    // 接过来的（同格子同名的记录）：如果这是刚建的角色（认准时才知道），start_fresh 把它挪开
    st.inherit = loaded.as_ref().map(|l| l.0.len() + l.1.len() + l.2.len()).filter(|&n| n > 0).map(|n| (from, n));
    let (recs, sold, shop) = loaded.unwrap_or_default();
    st.recs = recs;
    // 武器商店上一批：先留着（容器照记录重建，稳定后在背包 / 仓库里的算买走了、记进记录），这次进存档重新摆货
    st.shop_prev = shop.into_iter().filter(|r| r.cat == 0 && !st.recs.contains_key(&(r.cat, r.idx))).collect();
    st.shop = vec![None; d.items.wshop.len()];
    st.shop_init = false;
    // 没有回购商店（这次重随没开暗黑商店）就不留：容器照旧算空的
    st.sold = sold.into_iter().take(d.items.buyback.map_or(0, |b| b.n as usize)).map(Some).collect();
    st.last_held = None;
    st.bb_dirty = d.items.buyback.is_some();
    record_start(d, st, who, pgd);
    st.pending = None;
    if let Some(w) = st.gift_wait.take() {
        log_only_again(&w.log);
        GIFTS.lock().unwrap_or_else(|e| e.into_inner()).splice(0..0, w.items); // 读档前还没到的，重新排队
    }
    st.wish_queue = 0;
    st.more = false;
    st.batches = 0;
    st.over_max = if d.lib.rmax > 0 { over_max(d, pgd, d.lib.rmax) } else { HashSet::new() };
    if !st.over_max.is_empty() {
        log(&format!("新装备系统：{} 把生成武器已经强化到 +{} 以上（之前拿到的），强化照原版", st.over_max.len(), d.lib.rmax));
    }
    let (mut ok, mut bad) = (0, 0);
    let recs: Vec<Rec> = st.recs.values().chain(st.sold.iter().flatten()).chain(&st.shop_prev).cloned().collect();
    for r in &recs {
        match write_row(d, st, r) {
            Ok(()) => ok += 1,
            Err(e) => {
                bad += 1;
                if bad <= 3 {
                    log(&format!("新装备系统：重建 {}{} 失败：{e}", CAT_ZH[r.cat], rec_id(&d.items, r)));
                }
            }
        }
    }
    ashes::on_load(who); // 骨灰容器照这个角色的记录重建（gear/ashes.rs）
    let lot = lot_write(&d.items, &[]);
    flag_set(d.items.deliver_flag, false);
    flag_set(d.items.identify_flag, false);
    let spawned = unsafe { WorldChrMan::instance() }.ok().is_some_and(|w| w.main_player.is_some());
    log(&format!(
        "新装备系统：进存档（格子 {}，{}）→ 按记录重建 {ok} 件{}；人物{}生成；发货表{}",
        who.0,
        who.1,
        if bad > 0 { format!("（{bad} 件失败）") } else { String::new() },
        if spawned { "已经" } else { "还没" },
        match lot {
            Ok(()) => "已清空".to_string(),
            Err(e) => format!("没清空：{e}"),
        }
    ));
    texts_fresh();
}

/// 刚建的角色（who.rs：认准时游戏时间不到 3 分钟；用户 2026-11-19「收拢」「按游戏时间认」）：不接格子里原来的记录
/// （同格子同名的旧角色的，或者从旧记录里找回的同名的）。原来格子里那份复制进 旧记录、记录清空，开局装备的参数行和文字照全局
/// 那份（建角色界面看到的）重写，再把身上的开局装备记进来、写记录。骨灰同样（ashes::start_fresh）；这个格子的锁定名单清掉
fn start_fresh(d: &'static Defs, st: &mut St, who: &(i32, String), pgd: &PlayerGameData) {
    lock_clear_slot(who.0);
    ashes::start_fresh(who);
    let Some((from, n)) = st.inherit.take() else {
        log(&format!("新装备系统：格子 {}「{}」是刚建的角色，格子里没有要接的记录", who.0, who.1));
        return;
    };
    let path = rec_path(who.0);
    // 接的是这个格子的文件（同名）：复制一份进旧记录再盖掉；接的是旧记录里的一份：它本来就在那里，格子的文件写的时候照常另存
    let kept = if from == path.display().to_string() { rec_aside_copy(&path) } else { None };
    st.recs.clear();
    st.sold.clear();
    st.shop_prev.clear();
    st.bb_dirty = d.items.buyback.is_some();
    let starts = start_recs(d);
    let mut bad = 0;
    for r in starts {
        if write_row(d, st, r).is_err() {
            bad += 1;
        }
        write_texts(d, r);
    }
    texts_fresh();
    log(&format!(
        "新装备系统：格子 {}「{}」是刚建的角色，不接 {from} 里的 {n} 件{}；开局装备照建角色时的那份重写 {} 件{}",
        who.0,
        who.1,
        kept.map_or(String::new(), |p| format!("（原来那份复制到 {}）", p.display())),
        starts.len() - bad,
        if bad > 0 { format!("（{bad} 件失败）") } else { String::new() }
    ));
    record_start(d, st, who, pgd);
    rec_save(who, st); // 身上没有要记的开局装备时 record_start 不写：这里写一次，盖掉格子里的旧记录
}

/// 开局装备 / 追忆换的武器（items.txt 的 start 行）：身上 / 仓库里有、记录里还没有的，记下现在这一份
/// （以后点「一次性重随」也不影响这个角色）。进存档时记一次，在游戏里每 5 秒再看一次（追忆武器是中途换到的）
fn record_start(d: &Defs, st: &mut St, who: &(i32, String), pgd: &PlayerGameData) {
    let recs = start_recs(d);
    if recs.iter().all(|r| mine(st, (r.cat, r.idx))) {
        return;
    }
    let now = held(d, pgd);
    let new: Vec<Rec> = recs.iter().filter(|r| now.contains(&(r.cat, r.idx)) && !mine(st, (r.cat, r.idx))).cloned().collect();
    if !new.is_empty() {
        log(&format!("新装备系统：开局装备 / 追忆武器 {} 件记进这个角色的记录", new.len()));
        let stats = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
        for mut r in new {
            // 拿到时按人物现在的属性生成的（大蛇狩猎矛）：重新生成，改写参数行和文字
            let spec = d.items.start.iter().find(|s| s.pick && s.cat == r.cat && start_off(&d.items, s.cat, s.id) == Some(r.idx));
            if let Some(recipe) = spec.and_then(|s| pick_recipe(&d.lib, s, stats)) {
                r.recipe = recipe;
                match write_row(d, st, &r) {
                    Ok(()) => {
                        write_texts(d, &r);
                        log(&format!("新装备系统：{} 按人物现在的属性 {stats:?} 生成：{}", rec_id(&d.items, &r), rec_line(&r)));
                    }
                    Err(e) => log(&format!("新装备系统：{} 按属性生成后改写失败：{e}", rec_id(&d.items, &r))),
                }
            }
            st.recs.insert((r.cat, r.idx), r);
        }
        rec_save(who, st);
    }
}

/// 拿到时按人物现在的属性生成（start 行 pick = 1）：主补正放在最高的属性上（并列取前面的），能力需求不超过现在的属性；
/// 种子 = start 行的种子和属性混在一起（同样的属性生成一样的）
fn pick_recipe(lib: &Lib, s: &StartSpec, stats: [u32; 5]) -> Option<Recipe> {
    let maxreq = stats.map(|v| v.min(99) as u8);
    let hi = *maxreq.iter().max()?;
    let main = maxreq.iter().position(|&v| v == hi)? as i32;
    let seed = stats.iter().fold(s.seed, |h, &v| h.rotate_left(7) ^ v as u64);
    gen_weapon_on(lib, s.tier as usize, Some(WFix { maxreq, main, ..s.wfix() }), &mut Rng(seed))
}

/// 文字：进存档后把记录里的和正在发的都交一遍
fn texts_tick(d: &Defs, st: &St) {
    let mut g = TXT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(t) = g.as_mut() else { return };
    if !t.fresh {
        return;
    }
    t.fresh = false;
    let global = start_recs(d).iter().filter(|r| !mine(st, (r.cat, r.idx)));
    let all = st
        .recs
        .values()
        .chain(st.sold.iter().flatten())
        .chain(st.shop.iter().flatten())
        .chain(st.pending.iter().flat_map(|p| p.items.iter().filter_map(|x| x.0.rec())))
        .chain(global);
    let n: usize = all.map(|r| write_texts(d, r)).sum();
    if !st.recs.is_empty() {
        log(&format!("新装备系统：文字交好了 {n} 条{}", if crate::msghook::hooked() { "" } else { "（取文字挂钩还没挂上）" }));
    }
}

/// 鉴定一批：每件挑空容器、生成、改写，写进发货表、点亮发货标记
fn identify_batch(d: &Defs, st: &mut St, pgd: &PlayerGameData) {
    let unid = unidentified(d, pgd);
    let total: u32 = unid.iter().map(|x| x.1).sum();
    if total == 0 {
        log("新装备系统：背包里没有未鉴定物品");
        st.more = false;
        return;
    }
    let build = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    let wants = unid.iter().flat_map(|&(goods, qty)| {
        let (cat, tier) = d.items.goods[&goods];
        let bias = if cat == 0 { Bias::Build(build) } else { Bias::None };
        std::iter::repeat_n(Want { cat, tier, bias, goods }, qty as usize)
    });
    let b = batch::run(d, st, pgd, Src::Identify, wants, u32::MAX, |_| 0);
    let full = b.full;
    if b.made.is_empty() {
        log(&format!("新装备系统：容器满了，这次没鉴定（放不下：武器 {} / 防具 {} / 护符 {}；卖掉或丢掉一些生成的装备就能空出来）", full[0], full[1], full[2]));
        st.more = false;
        return;
    }
    let Some(ok_items) = ship(d, st, b.items()) else {
        st.more = false;
        return;
    };
    st.more = total > ok_items.len() as u32 && !b.any_full();
    log(&format!(
        "新装备系统：第 {} 批鉴定 {} 件（{}），发货表 {}~{}{}",
        st.batches,
        ok_items.len(),
        batch::made_text(&ok_items),
        d.items.deliver_lot,
        d.items.deliver_lot + d.items.deliver_rows - 1,
        if b.any_full() { format!("；容器满了放不下：武器 {} / 防具 {} / 护符 {}", full[0], full[1], full[2]) } else { String::new() }
    ));
    st.pending = Some(Pending::new(ok_items));
}

/// 定向鉴定能用的这一类（0 武器 / 1 防具 / 2 护符）未鉴定物品：(道具, 个数, 档)，rar = 5 是全部稀有度
fn dir_unid(d: &Defs, pgd: &PlayerGameData, cat: usize, rar: usize) -> Vec<(u32, u32, usize)> {
    unidentified(d, pgd)
        .into_iter()
        .filter_map(|(g, n)| d.items.goods.get(&g).filter(|&&(c, tier)| c == cat && (rar >= 5 || tier == rar)).map(|&(_, tier)| (g, n, tier)))
        .collect()
}

/// 这些未鉴定物品里最便宜的一件多少卢恩（一件都没有是 None）
fn dir_min_cost(di: &DirId, cat: usize, unid: &[(u32, u32, usize)]) -> Option<u32> {
    unid.iter().filter(|x| x.1 > 0).map(|x| di.cost_of(cat, x.2)).min()
}

/// 菜单写的种类、序号 → 这次定向鉴定（种类 0 武器类型 / 1 防具部位 / 2 护符流派）；对不上是 None
fn dir_req(d: &Defs, di: &DirId, kind: usize, k: usize, rar: usize, build: [u32; 5]) -> Option<DirReq> {
    let bias = match kind {
        0 => Bias::Typed(*di.types.get(k)?, build),
        1 if k < DIR_SLOTS => Bias::Slot(k as u8),
        2 => {
            let name = di.archs.get(k)?;
            Bias::Arch(d.lib.arch_names.iter().position(|a| a == name)? as u8)
        }
        _ => return None,
    };
    Some(DirReq { cat: kind, bias, rar })
}

/// 定向鉴定一批：这一类、这个稀有度的未鉴定物品，按 req.bias 生成（武器本体从那个类型里挑并偏向加点、防具只出那个部位、
/// 护符只出那个流派），每件按它的档扣卢恩（便宜的先鉴定，只鉴定付得起的件数）。一件都没鉴定返回 false
fn directed_batch(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, req: DirReq) -> bool {
    st.more_dir = None;
    let Some(di) = d.items.dirid.as_ref() else { return false };
    let cat = req.cat.min(2);
    let mut unid = dir_unid(d, pgd, cat, req.rar);
    unid.sort_by_key(|x| (di.cost_of(cat, x.2), x.0));
    let total: u32 = unid.iter().map(|x| x.1).sum();
    let Some(cheapest) = dir_min_cost(di, cat, &unid).filter(|&c| c <= pgd.rune_count) else {
        log(&format!("定向鉴定：{}，没鉴定（未鉴定{} {total} 件，卢恩 {}，各档价格 {:?}）",
                     if total == 0 { format!("背包里没有这种稀有度的未鉴定{}", DIR_CAT_ZH[cat]) } else { "卢恩不够".to_string() }, DIR_CAT_ZH[cat],
                     pgd.rune_count, if cat == 2 { di.cost_t } else { di.cost }));
        return false;
    };
    // 武器偏向现在的加点（排队的请求按发货时的）
    let build = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    let bias = match req.bias {
        Bias::Typed(t, _) => Bias::Typed(t, build),
        b => b,
    };
    let wants = unid.iter().flat_map(|&(goods, qty, tier)| std::iter::repeat_n(Want { cat, tier, bias, goods }, qty as usize));
    let b = batch::run(d, st, pgd, Src::Directed, wants, pgd.rune_count, |w| di.cost_of(cat, w.tier));
    if b.made.is_empty() {
        if b.full[cat] > 0 {
            log(&format!("定向鉴定：{0}容器满了，这次没鉴定（卖掉或丢掉一些生成的{0}就能空出来）", DIR_CAT_ZH[cat]));
        } else if b.failed > 0 {
            log(&format!("定向鉴定：{} 件生成不了（这个稀有度没有能出的{}），没鉴定", b.failed, DIR_CAT_ZH[cat]));
        }
        return false;
    }
    let Some(ok_items) = ship(d, st, b.items()) else { return false };
    let n = ok_items.len() as u32;
    let pay: u32 = ok_items.iter().map(|x| di.cost_of(cat, x.0.tier())).sum();
    pgd.rune_count = pgd.rune_count.saturating_sub(pay);
    // 还有没鉴定的、最便宜的那件还付得起（这一批鉴定的还在背包里，按剩下的件数比）：发完这批接着鉴定；有生成不了的就不接着，免得一直重试
    if total > n && b.full[cat] == 0 && b.failed == 0 && cheapest <= pgd.rune_count {
        st.more_dir = Some(DirReq { bias, ..req });
    }
    log(&format!("定向鉴定：第 {} 批，{}（{:?}）、稀有度 {}，鉴定 {n} 件（{}），扣 {pay} 卢恩（还剩 {}）{}", st.batches, DIR_CAT_ZH[cat], bias,
                 if req.rar >= 5 { "全部".to_string() } else { req.rar.to_string() }, batch::made_text(&ok_items), pgd.rune_count,
                 if st.more_dir.is_some() { "，接着鉴定下一批" } else { "" }));
    st.pending = Some(Pending::new(ok_items));
    true
}

/// 生成装备写容器（参数行、文字），和固定物品一起写进发货表、点亮发货标记；返回写成功的（一件都没成功 / 发货表写不了就 None）。
/// 调用的记成等待发货（Pending）
fn ship(d: &Defs, st: &mut St, items: Vec<(Made, u32)>) -> Option<Vec<(Made, u32)>> {
    let mut rows = Vec::new();
    let mut ok_items = Vec::new();
    {
        for (made, goods) in items {
            match &made {
                Made::Gear(rec) => match write_row(d, st, rec) {
                    Ok(()) => {
                        write_texts(d, rec);
                        rows.push((LOT_CAT[rec.cat], d.items.cont[rec.cat].id(rec.idx) as i32, 1));
                    }
                    Err(e) => {
                        log(&format!("新装备系统：改写容器失败：{e}"));
                        continue;
                    }
                },
                Made::Item { lot, id, n, .. } => rows.push((*lot, *id, *n)),
            }
            ok_items.push((made, goods));
        }
    }
    if ok_items.is_empty() {
        return None;
    }
    if let Err(e) = lot_write(&d.items, &rows) {
        log(&format!("新装备系统：改写发货表失败：{e}"));
        return None;
    }
    if !flag_set(d.items.deliver_flag, true) {
        log("新装备系统：点亮发货标记失败");
        return None;
    }
    st.batches += 1;
    Some(ok_items)
}

// ------------------------------------------------------------------ 向星辰许愿
fn player_block() -> Option<(u8, u8, u8, u8)> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let id = wcm.main_player.as_ref()?.current_block_id;
    Some((id.area(), id.block(), id.region(), id.index()))
}

/// 人物所在地图的档（找不到沿用上一次找到的，再没有就第 0 档）
fn wish_band(w: &Wish, st: &St) -> usize {
    player_block().and_then(|(a, b, r, i)| w.band_at(a, b, r, i)).or(st.wish_band).unwrap_or(0).min(w.bands.len() - 1)
}

/// 免费许愿还剩几次：从 free0 起连续点亮的标记是用掉的（dll\dark_sell 按记录补点亮）
fn wish_free_left(w: &Wish) -> u32 {
    w.free_n - (0..w.free_n).take_while(|&j| flag_get(w.free0 + j) == Some(true)).count() as u32
}

/// 再许 1 次付不付得起：排在前面还没许的 queued 次先用掉免费次数，剩下的按这个地区的价格算
fn wish_affordable(free_left: u32, queued: u32, runes: u32, cost: u32) -> bool {
    let paid = (queued + 1).saturating_sub(free_left);
    paid == 0 || runes as u64 >= cost as u64 * paid as u64
}

/// 许愿 n 次：先用免费次数，再扣卢恩（不够就到此为止）；每次按类别权重和这一档的稀有度生成一件成品装备（武器偏向加点），
/// 和鉴定一样发货。容器满了的那类不抽、不扣钱
fn wish_batch(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, n: u32) {
    let Some(w) = d.items.wish.as_ref() else { return };
    let b = wish_band(w, st);
    let band = w.bands[b];
    let left0 = wish_free_left(w);
    let used0 = w.free_n - left0;
    let mut rng = Rng::new();
    let build = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    // 每次按类别权重和这一档的稀有度抽（武器偏向加点）；先用免费次数，再按这个地区的价格收卢恩
    let wants = (0..n.min(d.items.deliver_rows)).map_while(|_| {
        let cat = *choose_weighted(&mut rng, &w.share, 1).first()?;
        let r = *choose_weighted(&mut rng, &band.w, 1).first()?;
        let tier = if r == 3 && band.dlc { 4 } else { r };
        Some(Want { cat, tier, bias: if cat == 0 { Bias::Build(build) } else { Bias::None }, goods: 0 })
    });
    let mut left = left0;
    let bt = batch::run(d, st, pgd, Src::Wish, wants, pgd.rune_count, |_| {
        if left > 0 {
            left -= 1;
            0
        } else {
            band.cost
        }
    });
    let n_free = bt.made.iter().filter(|x| x.2 == 0).count() as u32;
    let full: u32 = bt.full.iter().sum();
    if bt.made.is_empty() {
        log(&format!("向星辰许愿：{n} 次一件都没许成（{}）", if bt.poor { "卢恩不够" } else { "容器满了" }));
        return;
    }
    let Some(ok) = ship(d, st, bt.items()) else { return };
    // 按发出去的件数收费：先算免费的
    let k = ok.len() as u32;
    let use_free = n_free.min(k);
    let pay = (k - use_free) * band.cost;
    for j in used0..used0 + use_free {
        flag_set(w.free0 + j, true);
    }
    pgd.rune_count = pgd.rune_count.saturating_sub(pay);
    log(&format!(
        "向星辰许愿：{k} 件（{}；第 {b} 档），免费 {use_free} 次、花了 {pay} 卢恩{}{}",
        batch::made_text(&ok),
        if k < n { format!("；没许的 {} 次", n - k) } else { String::new() },
        if full > 0 { format!("（容器满了 {full} 次）") } else if bt.poor { "（卢恩不够）".to_string() } else { String::new() }
    ));
    st.pending = Some(Pending::new(ok));
}

/// 发货：标记熄灭（公共事件发完）后再等 1 秒，到了的记进记录、扣掉未鉴定道具；15 秒还没熄就算没发
fn deliver_check(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, who: &(i32, String)) {
    let Some(p) = st.pending.as_mut() else { return };
    if p.off_at.is_none() && flag_get(d.items.deliver_flag) == Some(false) {
        p.off_at = Some(Instant::now());
    }
    let (off_at, t0) = (p.off_at, p.t0);
    match off_at {
        Some(t) if t.elapsed().as_secs_f32() >= 1.0 => {
            let p = st.pending.take().unwrap();
            let held = held(d, pgd);
            let mut used: HashMap<u32, u32> = HashMap::new();
            let (mut got, mut miss) = (0, 0);
            for (made, goods) in p.items {
                // 生成装备看容器到没到背包；固定物品（掉落池）标记熄了就算发了（发货表已经给出去）
                let arrived = match &made {
                    Made::Gear(rec) => held.contains(&(rec.cat, rec.idx)),
                    Made::Item { .. } => true,
                };
                if !arrived {
                    miss += 1;
                    continue;
                }
                if goods != 0 {
                    *used.entry(goods).or_default() += 1;
                }
                if let Made::Gear(rec) = made {
                    st.recs.insert((rec.cat, rec.idx), rec);
                }
                got += 1;
            }
            rec_save(who, st);
            let mut removed = 0;
            for (g, n) in &used {
                removed += remove_goods(pgd, *g, *n);
            }
            log(&format!(
                "新装备系统：第 {} 批到了 {got} 件{}，扣掉未鉴定道具 {removed} 个",
                st.batches,
                if miss > 0 { format!("（{miss} 件没到，容器空着，未鉴定道具没扣）") } else { String::new() }
            ));
            if miss > 0 {
                st.more = false;
            }
        }
        None if t0.elapsed().as_secs_f32() > 15.0 => {
            log("新装备系统：点亮发货标记 15 秒了还没熄：公共事件没发（游戏脚本覆盖包里没有发货事件？）；这批作废，未鉴定道具没扣");
            st.pending = None;
            st.more = false;
            let _ = lot_write(&d.items, &[]);
            flag_set(d.items.deliver_flag, false);
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ 回购商店
/// 回购的一步：回购商店里又出现在 now 里的（买回来、捡回来）挪回记录，那格空着；last 里有、now 没有、在记录里的（卖了、丢了）
/// 挪到最前面（空格挤紧，超过 n 格的挤掉，容器空出来）。返回 (买回, 卖掉, 挤掉)
fn bb_step(st: &mut St, last: Option<&HashSet<(usize, u32)>>, now: &HashSet<(usize, u32)>, n: usize) -> (usize, usize, usize) {
    let mut back = 0;
    for s in st.sold.iter_mut() {
        if s.as_ref().is_some_and(|r| now.contains(&(r.cat, r.idx))) {
            let r = s.take().unwrap();
            st.recs.insert((r.cat, r.idx), r);
            back += 1;
        }
    }
    let Some(last) = last else { return (back, 0, 0) };
    let mut gone: Vec<(usize, u32)> = last.iter().filter(|k| !now.contains(k) && st.recs.contains_key(k)).copied().collect();
    if gone.is_empty() {
        return (back, 0, 0);
    }
    gone.sort();
    let mut v: Vec<Option<Rec>> = gone.iter().filter_map(|k| st.recs.remove(k)).map(Some).collect();
    v.extend(st.sold.drain(..).filter(|s| s.is_some()));
    let dropped = v.len().saturating_sub(n);
    st.evicted.extend(v.drain(n.min(v.len())..).flatten());
    st.sold = v;
    (back, gone.len(), dropped)
}

/// 每 30 帧数一次背包 + 仓库，走一步回购；有变化就存记录、重写商店行
fn buyback_tick(d: &Defs, st: &mut St, pgd: &PlayerGameData, who: &(i32, String)) {
    let Some(bb) = d.items.buyback else { return };
    if st.frames == SETTLE_FRAMES {
        st.bb_dirty = true; // 存档里的标记可能比进存档时写的晚载入，稳定后再写一遍
    }
    if st.frames >= SETTLE_FRAMES && st.frames % 30 == 15 {
        let now = held(d, pgd);
        let last = st.last_held.take();
        if now.is_empty() && last.as_ref().is_some_and(|l| !l.is_empty()) {
            st.last_held = last; // 背包 + 仓库一下子全空：多半在读档，不算卖掉
        } else {
            let (back, gone, dropped) = bb_step(st, last.as_ref(), &now, bb.n as usize);
            st.last_held = Some(now);
            if back + gone > 0 {
                rec_save(who, st);
                if !st.evicted.is_empty() {
                    evict_save(who, &st.evicted);
                    st.evicted.clear();
                }
                st.bb_dirty = true;
                log(&format!(
                    "回购商店：卖掉 / 丢掉 {gone} 件放进来、买回 {back} 件{}；现在 {} 件",
                    if dropped > 0 { format!("，挤掉最早的 {dropped} 件") } else { String::new() },
                    st.sold.iter().flatten().count()
                ));
            }
        }
    }
    if st.bb_dirty {
        st.bb_dirty = !bb_write(d, st, bb);
    }
}

/// 「挤掉的装备_<格子>.txt」超过这么多字节就只留后一半
const EVICT_MAX: usize = 512 * 1024;

/// 回购商店挤掉的装备另存一份（角色记录文件夹里的「挤掉的装备_<格子>.txt」，追加，一行「时间、角色名、记录行」）：平时用不到；
/// 万一挤错了能照这个补回记录（2026-11-01 用户：建新角色那一刻上一个角色的 132 件被当成卖掉、挤掉，记录就这么丢了）。
/// 启动器的启动前备份、诊断包都会带上它（角色记录文件夹里的 .txt 都带）
fn evict_save(who: &(i32, String), recs: &[Rec]) {
    let p = data_dir().join(format!("挤掉的装备_{}.txt", who.0));
    let old = fs::read_to_string(&p).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().filter(|l| !l.starts_with('#')).collect();
    if old.len() > EVICT_MAX {
        lines.drain(..lines.len() / 2);
    }
    let mut out = String::from("# 回购商店挤掉的装备（新的在后）：时间、角色名、记录行。平时用不到，记录出错时照这个补回\n");
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
    let t = crate::now_text();
    for r in recs {
        out.push_str(&format!("{t}\t{}\t{}\n", who.1, rec_line(r)));
    }
    let _ = fs::create_dir_all(data_dir());
    if fs::write(&p, out).is_err() {
        log(&format!("回购商店：挤掉的 {} 件另存到 {} 失败", recs.len(), p.display()));
    }
}

/// 商店行和标记：第 k 格有装备就把第 k 行改成它（价格 = 出售价，限 1 件）、点亮上架标记；空格熄灭上架标记。
/// 库存计数整段清零（买过的那格计数是 1；计数占几位、从哪头算不确定，所以连后面 STOCK_BITS 个号一起清）
fn bb_write(d: &Defs, st: &St, bb: Buyback) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    for k in 0..bb.n {
        let item = st.sold.get(k as usize).and_then(|s| s.as_ref());
        if let Some(r) = item {
            let id = rec_id(&d.items, r);
            let price = match r.cat {
                0 => repo.get::<EquipParamWeapon>(id).map(|p| p.sell_value()),
                1 => repo.get::<EquipParamProtector>(id).map(|p| p.sell_value()),
                _ => repo.get::<EquipParamAccessory>(id).map(|p| p.sell_value()),
            };
            let Some(row) = repo.get_mut::<ShopLineupParam>(bb.row0 + k) else {
                log(&format!("回购商店：商店行 {} 不在游戏参数里（这次不是用重随的模式启动的？），不放", bb.row0 + k));
                return true;
            };
            row.set_equip_type(INV_CAT[r.cat]);
            row.set_equip_id(id as i32);
            row.set_value(price.unwrap_or(0).max(0));
            row.set_sell_quantity(1);
        }
        if !flag_set(bb.rel0 + k, item.is_some()) {
            return false;
        }
    }
    for f in bb.stock0..bb.stock0 + bb.n + STOCK_BITS - 1 {
        flag_set(f, false);
    }
    true
}

/// 暗黑商店的武器商店（items.txt 的 wshop 行：商店行、武器类型、档）。稳定后每 30 帧数一次背包 + 仓库：
/// 上一批（进存档时记录里的）和这一批里被买走的记进记录，那一格换一把新的；进存档后第一次把所有格子摆上货
/// （每次进存档换一批，用户 2026-10-27 定的）。以前没做这一步，商店里一直是重随放的占位原版武器（用户 2026-10-29 报）
fn shop_tick(d: &Defs, st: &mut St, pgd: &PlayerGameData, who: &(i32, String)) {
    if d.items.wshop.is_empty() || st.frames < SETTLE_FRAMES || st.frames % 30 != 20 {
        return;
    }
    let now = held(d, pgd);
    if now.is_empty() && !st.recs.is_empty() {
        return; // 背包 + 仓库还没载入（多半在读档），下次再看
    }
    let mut bought = 0;
    for r in std::mem::take(&mut st.shop_prev) {
        if now.contains(&(r.cat, r.idx)) {
            st.recs.entry((r.cat, r.idx)).or_insert(r);
            bought += 1;
        }
    }
    let mut want = Vec::new();
    for (k, s) in st.shop.iter_mut().enumerate() {
        if s.as_ref().is_some_and(|r| now.contains(&(r.cat, r.idx))) {
            let r = s.take().unwrap();
            st.recs.insert((r.cat, r.idx), r);
            bought += 1;
            want.push(k);
        } else if s.is_none() && !st.shop_init {
            want.push(k);
        }
    }
    st.shop_init = true;
    if bought == 0 && want.is_empty() {
        return;
    }
    let (made, fail) = shop_fill(d, st, &now, &want);
    rec_save(who, st);
    log(&format!(
        "武器商店：买走 {bought} 把（记进记录），摆上 {made} 把新的{}",
        if fail > 0 { format!("，{fail} 格没摆上（照旧卖原来那把）") } else { String::new() }
    ));
}

/// 武器商店的这几格各生成一把（这一行的类型、档），放进空容器（不在记录、回购、商店里，也不在背包 / 仓库里），
/// 改写容器、文字和商店行（价格照重随定的）。返回 (摆上的, 没摆上的)
fn shop_fill(d: &Defs, st: &mut St, now: &HashSet<(usize, u32)>, slots: &[usize]) -> (usize, usize) {
    let free = free(d, st, now)[0].iter().copied().filter(|k| !st.recs.contains_key(&(0, *k)) && !now.contains(&(0, *k))).collect::<Vec<_>>();
    let mut free = free.into_iter();
    let mut rng = Rng::new();
    let (mut ok, mut bad) = (0, 0);
    for &k in slots {
        let Some(&(row, typ, tier)) = d.items.wshop.get(k) else { continue };
        let Some(recipe) = gen_weapon_typed(&d.lib, tier as usize, typ, &mut rng) else {
            bad += 1;
            continue;
        };
        let Some(idx) = free.next() else {
            bad += 1;
            continue;
        };
        let rec = Rec { cat: 0, idx, tier, recipe };
        if let Err(e) = write_row(d, st, &rec) {
            if bad == 0 {
                log(&format!("武器商店：改写容器失败：{e}"));
            }
            bad += 1;
            continue;
        }
        if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
            if let Some((grp, sid)) = shop_sort(d, repo, typ) {
                if let Some(w) = repo.get_mut::<EquipParamWeapon>(rec_id(&d.items, &rec)) {
                    w.set_sort_group_id(grp);
                    w.set_sort_id(sid + tier as i32);
                }
            }
        }
        let Some(sr) = (unsafe { SoloParamRepository::instance_mut() }).ok().and_then(|repo| repo.get_mut::<ShopLineupParam>(row)) else {
            if bad == 0 {
                log(&format!("武器商店：商店行 {row} 不在游戏参数里（这次不是用重随的模式启动的？）"));
            }
            bad += 1;
            continue;
        };
        sr.set_equip_id(rec_id(&d.items, &rec) as i32);
        write_texts(d, &rec);
        st.shop[k] = Some(rec);
        ok += 1;
    }
    (ok, bad)
}

/// 武器商店里同一类型的几把按稀有度排：商店按物品的排序组、排序号排，本体的排序号各不相同（同类型每把差 1000），照抄就乱了
/// （用户 2026-10-29 报）。返回这类型本体的排序组和最小的排序号，排序号 = 它 + 档。买走后这一局照这个排，下次进存档照本体
fn shop_sort(d: &Defs, repo: &SoloParamRepository, typ: u16) -> Option<(u8, i32)> {
    d.lib
        .wbase
        .iter()
        .filter(|b| b.typ == typ)
        .filter_map(|b| repo.get::<EquipParamWeapon>(b.id))
        .map(|p| (p.sort_group_id(), p.sort_id()))
        .filter(|x| x.0 != 255)
        .min_by_key(|x| x.1)
}

/// 确认框里的「背包里几件、容器还剩多少」等：算好交给取文字钩子（占位「〔#I<种类> …〕」，种类 0~9、A~G = 10~16；msghook.rs）
fn confirm_tick(d: &Defs, st: &St, pgd: &PlayerGameData) {
    let n: u32 = unidentified(d, pgd).iter().map(|x| x.1).sum();
    let held = held(d, pgd);
    let f = free(d, st, &held);
    // [种类] = (中文, 英文)：0 一键鉴定；1 / 2 许愿 1 次 / 10 次（这个地区的价格、免费还剩几次）；
    // 3~6 装备回收 普通 / 稀有及以下 / 史诗及以下 / 传说及以下（sc/darkshop.py）。没有的那种是 None（占位不改）
    let mut tx: Vec<Option<(String, String)>> = vec![Some((
        format!("待鉴定 {n} 件；还能获得新装备 武器 {} 防具 {} 护符 {}", f[0].len(), f[1].len(), f[2].len()),
        format!("{n} to identify; room for {} weapons, {} armor, {} talismans", f[0].len(), f[1].len(), f[2].len()),
    ))];
    if let Some(w) = d.items.wish.as_ref() {
        let cost = w.bands[wish_band(w, st)].cost;
        let left = wish_free_left(w);
        let ten = cost * 10u32.saturating_sub(left);
        tx.push(Some((format!("每次 {cost} 卢恩，免费还剩 {left} 次"), format!("{cost} runes each, {left} free left"))));
        tx.push(Some((format!("共 {ten} 卢恩，免费还剩 {left} 次"), format!("{ten} runes in total, {left} free left"))));
    } else {
        tx.extend([None, None]);
    }
    if let Some(rc) = d.items.recycle {
        for (k, s) in recycle_counts(d, st, pgd, &rc) {
            tx.push(Some((format!("预计回收 {k} 把，得到 {s} 个重铸石"), format!("{k} weapons -> {s} reforge stones"))));
        }
    }
    // 7：定向鉴定（各稀有度的未鉴定武器件数；价格按稀有度不同，写在确认框的固定文字里）
    tx.resize(7, None);
    if d.items.dirid.is_some() {
        let mut c = [0u32; 5];
        for (_, k, tier) in dir_unid(d, pgd, 0, 5) {
            c[tier.min(4)] += k;
        }
        tx.push(Some((
            format!("未鉴定武器：普通 {} 稀有 {} 史诗 {} 传说 {} 幽影 {}", c[0], c[1], c[2], c[3], c[4]),
            format!("Unidentified: Common {} Rare {} Epic {} Legendary {} Shadow {}", c[0], c[1], c[2], c[3], c[4]),
        )));
    }
    // 8：许愿石升级（现有几个、等级；sc/wishstone.py）
    tx.resize(8, None);
    tx.push(crate::wishstone::confirm_text(pgd));
    // 9 / 10（占位写 #IA）：防具、护符鉴定（各稀有度的未鉴定防具 / 护符件数）
    tx.resize(9, None);
    for (cat, zh) in [(1usize, "未鉴定防具"), (2, "未鉴定护符")] {
        tx.push(d.items.dirid.as_ref().map(|_| {
            let mut c = [0u32; 5];
            for (_, k, tier) in dir_unid(d, pgd, cat, 5) {
                c[tier.min(4)] += k;
            }
            (format!("{zh}：普通 {} 稀有 {} 史诗 {} 传说 {} 幽影 {}", c[0], c[1], c[2], c[3], c[4]),
             format!("Unidentified: Common {} Rare {} Epic {} Legendary {} Shadow {}", c[0], c[1], c[2], c[3], c[4]))
        }));
    }
    // 11（占位写 #IB）：卢恩银行的余额、身上的卢恩（runebank.rs）
    tx.resize(11, None);
    tx.push(crate::runebank::confirm_text(pgd));
    // 12~16（占位写 #IC~#IG）：物品菜单「送给 <队友>」（gear/gift.rs）
    tx.resize(12, None);
    for i in 0..gift::MAX_PEERS {
        tx.push(gift::label(i));
    }
    // 17（占位写 #IH）：回收全部弃置（「超限」页武器栏打了弃置标记的那些，用户 2026-11-26）
    tx.resize(17, None);
    if let Some(rc) = d.items.recycle.filter(|r| r.discard != 0) {
        let (k, s) = recycle_discard_counts(d, st, pgd, &rc);
        tx.push(Some(if k == 0 {
            ("没有可被回收的弃置武器".to_string(), "No discarded armaments to recycle".to_string())
        } else {
            (format!("预计回收 {k} 把弃置武器，得到 {s} 个重铸石"), format!("{k} discarded armaments -> {s} reforge stones"))
        }));
    }
    let key = tx.iter().map(|x| x.as_ref().map_or_else(String::new, |(a, b)| format!("{a}|{b}"))).collect::<Vec<_>>().join("|");
    let mut g = TXT.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.get_or_insert_with(Txt::default);
    if key == t.confirm_last {
        return;
    }
    t.confirm_last = key;
    for (kind, x) in tx.iter().enumerate() {
        if let Some((zh, en)) = x {
            let (zh, en): (Vec<u16>, Vec<u16>) = (zh.encode_utf16().collect(), en.encode_utf16().collect());
            crate::msghook::set_confirm(b'I', kind as u8, &zh, &en);
        }
    }
}

// ------------------------------------------------------------------ 武器重铸石、装备回收（rr/reforge.py，用户 2026-10-30）
/// 装备工坊「重铸」（用户 2026-10-31：只在工坊里重铸，背包里直接用重铸石重铸右手武器的路去掉了）：背包里这个角色的生成武器（配方序号 k）
/// 按它的稀有度重抽补正和能力需求（reforge_recipe，偏向现在的加点），改写参数行和文字、记进记录，再扣一个重铸石。
/// 先改参数行、再扣石头：扣不掉就把参数行改回去
fn reforge_apply(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, who: &(i32, String), k: u32) -> Result<[String; 2], [String; 2]> {
    let e = |zh: &str, en: &str| [zh.to_string(), en.to_string()];
    let rf = d.items.reforge.ok_or_else(|| e("这次没开武器重铸石", "Weapon Reforge Stones are disabled"))?;
    let old = st.recs.get(&(0, k)).cloned().ok_or_else(|| e("不是你的生成武器", "this is not your generated armament"))?;
    if matches!(old.recipe, Recipe::W { ovl: true, .. }) {
        return Err(e("这把武器超限过，不能再重铸", "this armament has been overlimited and can no longer be reforged"));
    }
    let base = rec_id(&d.items, &old);
    let held = pgd.equipment.equip_inventory_data.items_data.normal_entries().iter().filter_map(|x| x.as_option()).any(|it| {
        it.item_id.category() as u8 == INV_CAT[0] && it.quantity > 0 && it.item_id.param_id() / 10000 * 10000 == base
    });
    if !held {
        return Err(e("这把武器不在背包里了", "the armament is no longer in your inventory"));
    }
    if count_goods(pgd, rf.goods) < 1 {
        return Err(e("没有武器重铸石", "you have no Weapon Reforge Stone"));
    }
    let stats = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    let recipe = reforge_recipe(&d.lib, old.tier as usize, &old.recipe, Some(stats), &mut Rng::new()).ok_or_else(|| e("不是武器配方", "not an armament recipe"))?;
    let new = Rec { recipe, ..old.clone() };
    write_row(d, st, &new).map_err(|x| [format!("改写失败：{x}"), format!("failed to rewrite: {x}")])?;
    if remove_goods(pgd, rf.goods, 1) != 1 {
        let _ = write_row(d, st, &old);
        return Err(e("扣不掉武器重铸石，已改回原样", "could not use a Weapon Reforge Stone; nothing changed"));
    }
    write_texts(d, &new); // 全能（五项补正）会写进名字
    log(&format!("武器重铸：{base} 重铸（属性 {stats:?}）\n  旧 {}\n  新 {}", rec_line(&old), rec_line(&new)));
    let names = [plain_name(d, &new, 0), plain_name(d, &new, 1)];
    st.recs.insert((new.cat, new.idx), new);
    rec_save(who, st);
    Ok([format!("「{}」重铸完成。", names[0]), format!("\"{}\" has been reforged.", names[1])])
}

/// 装备着的：（物品编号原始值, 背包格序号）（同 dark_sell）
fn equipped(pgd: &PlayerGameData) -> (HashSet<u32>, HashSet<u32>) {
    let e = &pgd.equipment.equipment_entries;
    let ids = [
        e.weapon_primary_left.into_inner(),
        e.weapon_primary_right.into_inner(),
        e.weapon_secondary_left.into_inner(),
        e.weapon_secondary_right.into_inner(),
        e.weapon_tertiary_left.into_inner(),
        e.weapon_tertiary_right.into_inner(),
    ];
    (ids.into_iter().collect(), pgd.equipment.equipment_item_idx_list.iter().copied().collect())
}

/// 装备回收的候选：背包里（不含仓库）这个角色的生成武器，没装备着、没锁定的 →（背包格序号, 物品编号原始值, (类, 序号), 稀有度 0~3）
fn recycle_list(d: &Defs, st: &St, pgd: &PlayerGameData) -> Vec<(usize, u32, (usize, u32), usize)> {
    let (eq_ids, eq_idx) = equipped(pgd);
    let inv = &pgd.equipment.equip_inventory_data;
    let key_cap = inv.items_data.key_items_capacity;
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    let mut out = Vec::new();
    for (idx, e) in inv.items_data.normal_entries().iter().enumerate() {
        let Some(it) = e.as_option() else { continue };
        if it.item_id.category() as u8 != INV_CAT[0] || it.quantity == 0 {
            continue;
        }
        let raw = it.item_id.into_inner();
        if eq_idx.contains(&(key_cap + idx as u32)) || eq_ids.contains(&raw) {
            continue;
        }
        let base = it.item_id.param_id() / 10000 * 10000;
        let Some(k) = d.items.cont[0].idx_of(base).or_else(|| start_off(&d.items, 0, base)) else { continue };
        let Some(rec) = st.recs.get(&(0, k)) else { continue };
        if !locked.contains(&(0, base)) {
            out.push((idx, raw, (0, k), rec.rar()));
        }
    }
    out
}

/// 确认框用：各档（回收 普通 / 稀有及以下 / 史诗及以下 / 传说及以下）回收几把、得几个重铸石
fn recycle_counts(d: &Defs, st: &St, pgd: &PlayerGameData, rc: &Recycle) -> [(u32, u32); 4] {
    let mut out = [(0u32, 0u32); 4];
    for (_, _, _, rar) in recycle_list(d, st, pgd) {
        let r = rar.min(3);
        for x in out.iter_mut().skip(r) {
            x.0 += 1;
            x.1 += rc.stones[r];
        }
    }
    out
}

/// 装备回收（暗黑商店）：这一档及以下的候选逐把移除（游戏自己的移除函数，同一键出售），移掉的从记录里删掉（不进回购商店），
/// 按稀有度给的重铸石借发货表发
fn recycle_batch(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, who: &(i32, String), rc: Recycle, t: usize) {
    let list: Vec<_> = recycle_list(d, st, pgd).into_iter().filter(|x| x.3 <= t).collect();
    if list.is_empty() {
        log(&format!("装备回收：档 {t} 及以下没有能回收的生成武器"));
        return;
    }
    let Some(f) = remove_item_fn() else { return };
    let Some(inv_ptr) = inventory(pgd) else {
        log("装备回收：背包结构地址对不上，没回收");
        return;
    };
    let inv = unsafe { &mut *inv_ptr };
    let key_cap = inv.items_data.key_items_capacity;
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    let here = |inv: &EquipInventoryData, idx: usize, raw: u32| inv.items_data.normal_entries()[idx].as_option().is_some_and(|it| it.item_id.into_inner() == raw);
    let (mut n, mut stones, mut fail) = (0u32, 0u32, 0u32);
    for (idx, raw, key, rar) in list {
        if !here(inv, idx, raw) {
            fail += 1;
            continue;
        }
        unsafe { remove(inv_ptr, key_cap + idx as u32, 1) };
        if here(inv, idx, raw) {
            fail += 1;
            continue;
        }
        st.recs.remove(&key);
        n += 1;
        stones += rc.stones[rar.min(3)];
    }
    if n > 0 {
        rec_save(who, st);
    }
    let sent = stones == 0 || gift_n(1, rc.goods as i32, stones);
    log(&format!(
        "装备回收：档 {t} 及以下回收 {n} 把，重铸石 {stones} 个{}{}",
        if sent { "已排队发" } else { "没发（没有发货表）" },
        if fail > 0 { format!("；{fail} 把没移掉（格子变了或移除失败）") } else { String::new() }
    ));
}

/// 回收全部弃置（用户 2026-11-26）：背包里（不含仓库）这个角色的生成武器、没装备着、没锁定、且在弃置名单里的，
/// 逐把移除（和 recycle_batch 同一套：游戏自己的移除函数 + 删记录 + 借发货表发重铸石），成功后从弃置名单删掉
fn recycle_discarded(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, who: &(i32, String), rc: Recycle) {
    let disc = discard_keys(&discard_text(), who);
    let list: Vec<_> = recycle_list(d, st, pgd).into_iter().filter(|x| disc.contains(&((x.1 & 0x0FFF_FFFF) / 10000 * 10000))).collect();
    if list.is_empty() {
        log("装备回收（弃置）：没有能回收的弃置武器（背包里、没装备、没锁定的才算）");
        return;
    }
    let Some(f) = remove_item_fn() else { return };
    let Some(inv_ptr) = inventory(pgd) else {
        log("装备回收（弃置）：背包结构地址对不上，没回收");
        return;
    };
    let inv = unsafe { &mut *inv_ptr };
    let key_cap = inv.items_data.key_items_capacity;
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    let here = |inv: &EquipInventoryData, idx: usize, raw: u32| inv.items_data.normal_entries()[idx].as_option().is_some_and(|it| it.item_id.into_inner() == raw);
    let (mut n, mut stones, mut fail, mut done) = (0u32, 0u32, 0u32, Vec::new());
    for (idx, raw, key, rar) in list {
        if !here(inv, idx, raw) {
            fail += 1;
            continue;
        }
        unsafe { remove(inv_ptr, key_cap + idx as u32, 1) };
        if here(inv, idx, raw) {
            fail += 1;
            continue;
        }
        st.recs.remove(&key);
        done.push((raw & 0x0FFF_FFFF) / 10000 * 10000);
        n += 1;
        stones += rc.stones[rar.min(3)];
    }
    if n > 0 {
        rec_save(who, st);
    }
    discard_remove(who, &done); // 回收掉的当场从名单删，不等下次修剪
    let sent = stones == 0 || gift_n(1, rc.goods as i32, stones);
    log(&format!(
        "装备回收（弃置）：回收 {n} 把，重铸石 {stones} 个{}{}",
        if sent { "已排队发" } else { "没发（没有发货表）" },
        if fail > 0 { format!("；{fail} 把没移掉（格子变了或移除失败）") } else { String::new() }
    ));
}

/// 确认框用：回收全部弃置能回收几把、得几个重铸石
fn recycle_discard_counts(d: &Defs, st: &St, pgd: &PlayerGameData, rc: &Recycle) -> (u32, u32) {
    let disc = st.who.as_ref().map(|w| discard_keys(&discard_text(), w)).unwrap_or_default();
    let (mut n, mut s) = (0u32, 0u32);
    for (_, raw, _, rar) in recycle_list(d, st, pgd) {
        if disc.contains(&((raw & 0x0FFF_FFFF) / 10000 * 10000)) {
            n += 1;
            s += rc.stones[rar.min(3)];
        }
    }
    (n, s)
}

// ------------------------------------------------------------------ 装备继承（装备工坊的「继承」页，用户 2026-10-31）
// 界面在 dll\dark_ui（赐福「装备工坊」打开），数据和执行接口在 gear\workshop.rs，这里是规则：来源是背包里没穿着、没锁定的生成防具，
// 目标是背包里任意生成防具；机制类进前缀位、数值类进后缀位，有空位补上（最多 1 前缀 + 2 后缀），满了替换其中一条；
// 词条保留来源的稀有度；来源销毁、另扣 1 个融锻之心（inherit 行的道具，Boss 掉落，用户 2026-10-31 从武器重铸石改过来）。
// 也能提取成同一词条的护符（稀有度同这条词条）
const IH_PART: [[&str; 4]; 2] = [["头部", "身体", "手部", "腿部"], ["Head", "Chest", "Arms", "Legs"]];

fn fam_mech(lib: &Lib, key: &str) -> bool {
    lib.fam_by_key.get(key).is_some_and(|&i| lib.fams[i].mech)
}

fn same_group(lib: &Lib, a: &str, b: &str) -> bool {
    let g = |k: &str| lib.fam_by_key.get(k).map(|&i| lib.fams[i].group.clone()).unwrap_or_default();
    let ga = g(a);
    !ga.is_empty() && ga == g(b)
}

/// 防具的物品编号 → 容器 / 开局装备的序号
fn armor_key(d: &Defs, pid: u32) -> Option<u32> {
    d.items.cont[1].idx_of(pid).or_else(|| start_off(&d.items, 1, pid))
}

/// 穿着的头 / 身 / 手 / 腿的物品编号
/// 身上穿戴的装备现在提供的常驻效果：防具三格、护符四格 + 第五格（refId 和常驻）、六把武器的常驻（去掉强化等级查本体行）
fn worn_providers(pgd: &PlayerGameData) -> HashSet<i32> {
    let mut out = HashSet::new();
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return out };
    let e = &pgd.equipment.equipment_entries;
    for id in [e.protector_head, e.protector_chest, e.protector_hands, e.protector_legs] {
        if let Some(r) = repo.get::<EquipParamProtector>(id.param_id()) {
            out.extend([r.resident_sp_effect_id(), r.resident_sp_effect_id2(), r.resident_sp_effect_id3()]);
        }
    }
    for id in e.accessories.iter().chain(std::iter::once(&e.covenant)).filter_map(|x| x.as_valid()) {
        if let Some(r) = repo.get::<EquipParamAccessory>(id.param_id()) {
            out.extend([r.ref_id(), r.resident_sp_effect_id1(), r.resident_sp_effect_id2(), r.resident_sp_effect_id3(), r.resident_sp_effect_id4()]);
        }
    }
    for id in [e.weapon_primary_left, e.weapon_primary_right, e.weapon_secondary_left, e.weapon_secondary_right, e.weapon_tertiary_left, e.weapon_tertiary_right] {
        let pid = id.param_id();
        if let Some(r) = repo.get::<EquipParamWeapon>(pid - pid % 100) {
            out.extend([r.resident_sp_effect_id(), r.resident_sp_effect_id1(), r.resident_sp_effect_id2()]);
        }
    }
    out.retain(|&s| s > 0);
    out
}

/// 被换掉 / 新加的常驻效果 → 身上该摘的、该挂的：换掉的没有别的装备再提供、身上还挂着才摘；新加的身上没有才挂
fn worn_delta(old: &[i32], new: &[i32], still: &HashSet<i32>, have: &HashSet<i32>) -> (Vec<i32>, Vec<i32>) {
    let gone = old.iter().copied().filter(|s| *s > 0 && !new.contains(s) && !still.contains(s) && have.contains(s)).collect();
    let put = new.iter().copied().filter(|s| *s > 0 && !old.contains(s) && !have.contains(s)).collect();
    (gone, put)
}

/// 改写了一件正穿着的装备（继承换词条、超限升词条）后把身上的常驻效果同步过来：游戏只在穿上 / 换下时按参数行「当时」的编号挂 / 摘，
/// 改写后换下摘的是新编号，被换掉的那条就一直挂在身上（积木词条还以为戴着；用户 2026-11-13「装备继承替换词条时没有正确清除被替换词条的speffect」）。
/// 这里当场摘旧挂新，以后换下时游戏摘的正好对得上。old / new = 这件装备改写前后的常驻效果；要在参数行改写之后调
/// 防具配方里挂在装备行上的效果（词条数值随机的不在装备行上，归 gear/valfx.rs 挂占位行；当场同步身上效果只管这些）
fn native_sps(lib: &Lib, rec: &Rec) -> Vec<i32> {
    match &rec.recipe {
        Recipe::P { sps, .. } => sps.iter().filter(|x| !is_val(lib, &x.1, sp_rar(lib, &x.1, x.0).unwrap_or(rec.rar()))).map(|x| x.0).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn sync_worn_effects(pgd: &PlayerGameData, old: &[i32], new: &[i32]) -> String {
    let still = worn_providers(pgd);
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return "人物还没生成，没同步".into() };
    let Some(p) = wcm.main_player.as_mut() else { return "人物还没生成，没同步".into() };
    let have: HashSet<i32> = p.chr_ins.special_effect.entries().map(|x| x.param_id).collect();
    let (gone, put) = worn_delta(old, new, &still, &have);
    for &s in &gone {
        p.remove_speffect(s);
    }
    for &s in &put {
        p.apply_speffect(s, false);
    }
    format!("身上的效果同步：摘掉 {gone:?}，挂上 {put:?}")
}

fn worn_armor(pgd: &PlayerGameData) -> [u32; 4] {
    let e = &pgd.equipment.equipment_entries;
    [e.protector_head.param_id(), e.protector_chest.param_id(), e.protector_hands.param_id(), e.protector_legs.param_id()]
}

/// 背包里某个道具一共几个
pub(crate) fn count_goods(pgd: &PlayerGameData, goods: u32) -> u32 {
    pgd.equipment
        .equip_inventory_data
        .items_data
        .normal_entries()
        .iter()
        .filter_map(|e| e.as_option())
        .filter(|it| it.item_id.category() as u8 == INV_GOODS && it.item_id.param_id() == goods)
        .map(|it| it.quantity)
        .sum()
}

/// 背包（不含仓库）里这一类、这个编号的各格 →（格序号, 物品编号原始值）
fn inv_slots(pgd: &PlayerGameData, cat: u8, pid: u32) -> Vec<(usize, u32)> {
    pgd.equipment
        .equip_inventory_data
        .items_data
        .normal_entries()
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.as_option().map(|it| (i, it)))
        .filter(|(_, it)| it.item_id.category() as u8 == cat && it.item_id.param_id() == pid && it.quantity > 0)
        .map(|(i, it)| (i, it.item_id.into_inner()))
        .collect()
}

/// 移掉背包第 idx 格（整格，游戏自己的移除函数）；移掉了返回 true
fn remove_slot(pgd: &mut PlayerGameData, idx: usize, raw: u32) -> bool {
    let Some(f) = remove_item_fn() else { return false };
    let Some(inv_ptr) = inventory(pgd) else { return false };
    let inv = unsafe { &mut *inv_ptr };
    let here = |inv: &EquipInventoryData| inv.items_data.normal_entries().get(idx).and_then(|e| e.as_option()).is_some_and(|it| it.item_id.into_inner() == raw);
    if !here(inv) {
        return false;
    }
    let key_cap = inv.items_data.key_items_capacity;
    let remove = unsafe { std::mem::transmute::<usize, RemoveItemFn>(f) };
    unsafe { remove(inv_ptr, key_cap + idx as u32, 1) };
    !here(inv)
}

/// 背包（不含仓库）里这个角色的一件生成防具
#[derive(Debug, Clone)]
struct IhArmor {
    pid: u32,
    idx: u32,
    rar: usize,
    /// 部位 0 头 / 1 身 / 2 手 / 3 腿
    part: usize,
    worn: bool,
    locked: bool,
    /// 获得顺序（背包格子的 sort_id，越大越新）
    sort: u32,
}

/// 背包里（不含仓库）这个角色的生成防具（一件一个，不重复），传说在前、同稀有度按部位
fn ih_armor(d: &Defs, st: &St, pgd: &PlayerGameData) -> Vec<IhArmor> {
    let (_, eq_idx) = equipped(pgd);
    let worn = worn_armor(pgd);
    let inv = &pgd.equipment.equip_inventory_data;
    let key_cap = inv.items_data.key_items_capacity;
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    let mut out: Vec<IhArmor> = Vec::new();
    for (idx, e) in inv.items_data.normal_entries().iter().enumerate() {
        let Some(it) = e.as_option() else { continue };
        if it.item_id.category() as u8 != INV_CAT[1] || it.quantity == 0 {
            continue;
        }
        let pid = it.item_id.param_id();
        if out.iter().any(|x| x.pid == pid) {
            continue;
        }
        let Some(k) = armor_key(d, pid) else { continue };
        let Some(rec) = st.recs.get(&(1, k)) else { continue };
        let Recipe::P { base, .. } = &rec.recipe else { continue };
        let part = d.lib.pbase.iter().find(|b| b.id == *base).map_or(9, |b| b.slot as usize);
        let is_worn = eq_idx.contains(&(key_cap + idx as u32)) || worn.contains(&pid);
        out.push(IhArmor { pid, idx: k, rar: rec.rar(), part, worn: is_worn, locked: locked.contains(&(1, pid)), sort: it.sort_id });
    }
    out.sort_by_key(|x| (std::cmp::Reverse(x.rar), x.part, x.idx));
    out
}

/// 来源第 k 条词条放到目标上的各种放法：Ok([(替换目标的第 i 条 / None = 补上, None = 能放 / Some(不能的原因 [中文, 英文]))])；
/// 整件都放不了是 Err(原因)
fn ih_options(lib: &Lib, src: &Rec, k: usize, tgt: &Rec) -> Result<Vec<(Option<usize>, Option<[String; 2]>)>, [String; 2]> {
    let e = |zh: String, en: String| [zh, en];
    let Recipe::P { sps, .. } = &src.recipe else { return Err(e("来源不是防具".into(), "the source is not armor".into())) };
    let Some((_, key)) = sps.get(k) else { return Err(e("来源没有这条词条".into(), "the source has no such affix".into())) };
    let Some(fam) = lib.fam_by_key.get(key).map(|&i| &lib.fams[i]) else { return Err(e("不认识这条词条".into(), "unknown affix".into())) };
    let Recipe::P { base, sps: tsps, .. } = &tgt.recipe else { return Err(e("目标不是防具".into(), "the target is not armor".into())) };
    let part = lib.pbase.iter().find(|b| b.id == *base).map_or(9, |b| b.slot as usize);
    if part > 3 || fam.slots & (1 << part) == 0 {
        let p = [IH_PART[0].get(part).copied().unwrap_or("这个部位"), IH_PART[1].get(part).copied().unwrap_or("this slot")];
        return Err(e(format!("这条词条不能出在{}", p[0]), format!("this affix cannot roll on {}", p[1])));
    }
    let mine: Vec<usize> = (0..tsps.len()).filter(|&i| fam_mech(lib, &tsps[i].1) == fam.mech).collect();
    let reps: Vec<Option<usize>> = match (fam.mech, mine.as_slice()) {
        (true, [i, ..]) => vec![Some(*i)],
        (false, [_, _, ..]) => mine.iter().map(|&i| Some(i)).collect(), // 后缀满了（老装备可能有三条数值类）：每条都能换
        _ if tsps.len() < 3 => vec![None],
        (false, [a]) => vec![Some(*a)],
        _ => return Err(e("目标已经有 3 条词条，没有能换的位置".into(), "the target already has 3 affixes and no matching slot".into())),
    };
    Ok(reps
        .into_iter()
        .map(|rep| {
            let kept = tsps.iter().enumerate().filter(|(i, _)| Some(*i) != rep).map(|(_, x)| &x.1);
            let why = kept.filter_map(|k2| {
                if k2 == key {
                    Some(e("目标已经有这条词条".into(), "the target already has this affix".into()))
                } else if same_group(lib, k2, key) {
                    Some(e(format!("目标已经有同组的「{}」", suffix(lib, k2, 0)), format!("the target already has \"{}\" from the same group", suffix(lib, k2, 1))))
                } else {
                    None
                }
            });
            (rep, why.into_iter().next())
        })
        .collect())
}

/// 这条词条能提取成的护符的模板（它的稀有度有护符才有）
fn ih_talisman(lib: &Lib, key: &str, r: usize) -> Option<i32> {
    lib.fam_by_key.get(key).and_then(|&i| lib.fams[i].r[r].as_ref()).map(|x| x.tal).filter(|&x| x >= 0)
}

/// 装备的名字（不带颜色标记）
fn plain_name(d: &Defs, rec: &Rec, lang: usize) -> String {
    strip_font(&compose_cap(&d.lib, rec, lang, d.items.cap[1])[0])
}

/// 执行：来源（类 1 的序号）的第 k 条词条放到目标上（target = None 是提取成护符；replace = 替换目标的第几条，None = 补上）。
/// 成了返回给界面看的结果 [中文, 英文]，不成返回原因
fn ih_apply(
    d: &Defs,
    st: &mut St,
    pgd: &mut PlayerGameData,
    who: &(i32, String),
    src_idx: u32,
    k: usize,
    target: Option<u32>,
    replace: Option<usize>,
) -> Result<[String; 2], [String; 2]> {
    let e = |zh: &str, en: &str| [zh.to_string(), en.to_string()];
    let src = st.recs.get(&(1, src_idx)).cloned().ok_or_else(|| e("来源不是你的生成装备", "the source is not your generated gear"))?;
    let Recipe::P { sps: ssps, pos: spos, .. } = &src.recipe else { return Err(e("来源不是防具", "the source is not armor")) };
    let (sp, key) = ssps.get(k).cloned().ok_or_else(|| e("来源没有这条词条", "the source has no such affix"))?;
    let p = pos_at(spos, k); // 词条数值随机：位置跟着词条走
    let src_id = rec_id(&d.items, &src);
    let Some(&(src_slot, src_raw)) = inv_slots(pgd, INV_CAT[1], src_id).first() else {
        return Err(e("来源不在背包里了", "the source is no longer in your inventory"));
    };
    if worn_armor(pgd).contains(&src_id) {
        return Err(e("来源穿在身上", "you are wearing the source"));
    }
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    if locked.contains(&(1, src_id)) {
        return Err(e("来源锁定了", "the source is locked"));
    }
    let stone = d.items.inherit.map(|i| i.goods).filter(|&g| g > 0).ok_or_else(|| e("这次没有融锻之心（重随没写）", "Fusion Hearts are missing (not written by the reroll)"))?;
    if count_goods(pgd, stone) < 1 {
        return Err(e("没有融锻之心（打倒 Boss 获得）", "you have no Fusion Heart (bosses drop them)"));
    }
    let suf = [suffix(&d.lib, &key, 0), suffix(&d.lib, &key, 1)];
    let done = match target {
        Some(t) => {
            if t == src_idx {
                return Err(e("来源和目标是同一件", "the source and the target are the same piece"));
            }
            let old = st.recs.get(&(1, t)).cloned().ok_or_else(|| e("目标不是你的生成装备", "the target is not your generated gear"))?;
            let tid = rec_id(&d.items, &old);
            if inv_slots(pgd, INV_CAT[1], tid).is_empty() {
                return Err(e("目标不在背包里了", "the target is no longer in your inventory"));
            }
            let opts = ih_options(&d.lib, &src, k, &old)?;
            match opts.iter().find(|o| o.0 == replace) {
                None => return Err(e("目标的词条变了，请重新选", "the target's affixes changed; choose again")),
                Some((_, Some(why))) => return Err(why.clone()),
                Some(_) => {}
            }
            let Recipe::P { base, sps: tsps, mult, pos: tpos } = &old.recipe else { return Err(e("目标不是防具", "the target is not armor")) };
            let old_sps: Vec<i32> = native_sps(&d.lib, &old); // 随机词条归 valfx（下一轮按新配方换占位行）
            let mut nsps: Vec<(i32, String, i16)> = tsps.iter().enumerate().map(|(i, x)| (x.0, x.1.clone(), pos_at(tpos, i))).collect();
            match replace {
                Some(i) => nsps[i] = (sp, key.clone(), p),
                None => nsps.push((sp, key.clone(), p)),
            }
            nsps.sort_by_key(|x| !fam_mech(&d.lib, &x.1)); // 机制类（前缀）在前
            let npos: Vec<i16> = if nsps.iter().any(|x| x.2 != 0) { nsps.iter().map(|x| x.2).collect() } else { Vec::new() };
            let nsps: Vec<(i32, String)> = nsps.into_iter().map(|x| (x.0, x.1)).collect();
            let new = Rec { recipe: Recipe::P { base: *base, sps: nsps, mult: *mult, pos: npos }, ..old.clone() };
            write_row(d, st, &new).map_err(|x| [format!("改写目标失败：{x}"), format!("failed to rewrite the target: {x}")])?;
            // 先扣融锻之心再移走来源（和超限、重铸一样）：扣不掉就改回目标、来源不动（以前扣在最后，扣的时候出错就只剩来源没了）
            if remove_goods(pgd, stone, 1) != 1 {
                let _ = write_row(d, st, &old);
                return Err(e("扣不掉融锻之心，目标已改回、来源没动", "could not use a Fusion Heart; the target was restored and the source kept"));
            }
            st.recs.remove(&(1, src_idx)); // 先删记录：移掉后不进回购商店
            if !remove_slot(pgd, src_slot, src_raw) {
                st.recs.insert((1, src_idx), src.clone());
                let _ = write_row(d, st, &old);
                gift_n(1, stone as i32, 1);
                return Err(e("来源移除失败，目标已改回、融锻之心已还回", "could not remove the source; the target was restored and the Fusion Heart returned"));
            }
            write_texts(d, &new);
            let tname = [plain_name(d, &new, 0), plain_name(d, &new, 1)];
            let worn = worn_armor(pgd).contains(&tid);
            log(&format!("装备继承：{} 的 {key} 转到 {tid}\n  旧 {}\n  新 {}", rec_line(&src), rec_line(&old), rec_line(&new)));
            if worn {
                let new_sps: Vec<i32> = native_sps(&d.lib, &new);
                log(&format!("装备继承：目标正穿着，{}", sync_worn_effects(pgd, &old_sps, &new_sps)));
            }
            st.recs.insert((1, t), new);
            [
                format!("「{}」已转到「{}」上。{}", suf[0], tname[0], if worn { "这件正穿着，已经生效。" } else { "" }),
                format!("\"{}\" was moved onto \"{}\".{}", suf[1], tname[1], if worn { " It is worn and already takes effect." } else { "" }),
            ]
        }
        None => {
            if st.pending.is_some() || st.gift_wait.is_some() {
                return Err(e("正在发放别的装备，稍后再试", "other gear is being delivered; try again shortly"));
            }
            let r = sp_rar(&d.lib, &key, sp).unwrap_or(src.rar());
            let tal = ih_talisman(&d.lib, &key, r).ok_or_else(|| e("这条词条没有护符", "this affix has no talisman"))?;
            let held = held(d, pgd);
            let idx = *free(d, st, &held)[2].first().ok_or_else(|| e("护符容器满了，卖掉或丢掉一些生成的护符", "no free talisman slots; sell or drop some generated talismans"))?;
            let rec = Rec { cat: 2, idx, tier: r as u8, recipe: Recipe::A { tal: tal as u32, sp, key: key.clone(), pos: p } };
            let tname = [plain_name(d, &rec, 0), plain_name(d, &rec, 1)];
            if remove_goods(pgd, stone, 1) != 1 {
                return Err(e("扣不掉融锻之心，没提取", "could not use a Fusion Heart; nothing was extracted"));
            }
            let Some(ok) = ship(d, st, vec![(Made::Gear(rec), 0)]) else {
                gift_n(1, stone as i32, 1);
                return Err(e("发放失败，融锻之心已还回", "delivery failed; the Fusion Heart was returned"));
            };
            st.pending = Some(Pending::new(ok));
            st.recs.remove(&(1, src_idx));
            if !remove_slot(pgd, src_slot, src_raw) {
                st.recs.insert((1, src_idx), src.clone());
                log("装备继承：护符已发，来源没移掉");
            }
            log(&format!("装备继承：{} 的 {key} 提取成护符（稀有度 {r}）", rec_line(&src)));
            [format!("提取出「{}」，稍后放进背包。", tname[0]), format!("Extracted \"{}\". It will arrive in your inventory shortly.", tname[1])]
        }
    };
    rec_save(who, st);
    Ok(done)
}

pub fn tick() {
    let Some(d) = defs() else { return };
    let mut g = ST.lock().unwrap_or_else(|e| e.into_inner());
    let st = g.get_or_insert_with(St::default);
    start_tick(d, st);
    frame_templates(d, st);
    start_texts(d, st);
    let Ok(gdm) = (unsafe { GameDataMan::instance_mut() }) else { return };
    let pgd: &mut PlayerGameData = &mut gdm.main_player_game_data;
    if pgd.level == 0 {
        if st.who.is_some() {
            // 回到主菜单：开局装备换回全局那份（建新角色时看到、拿到的是这次抽的）
            (st.start_rows, st.start_tries, st.start_wait) = (false, 0, 0);
        }
        if REC_OWED.swap(false, Ordering::Relaxed) {
            log("新装备系统：回到主菜单时角色还没进到世界里，它欠着的记录不写");
        }
        st.who = None;
        st.frames = 0;
        st.death = Default::default(); // 死亡惩罚欠着的不带到下一个角色
        return;
    }
    // 身份照 who.rs（格子 + 角色名；格子号是负的、名字还没读进来的那几帧照旧：以前用空名字也算进存档，把这个格子的记录
    // 按空名字覆盖掉了，用户 2026-10-28 报的）
    let Some(who) = crate::who::current() else { return };
    if st.who.as_ref() != Some(&who) {
        if REC_OWED.swap(false, Ordering::Relaxed) {
            // 上一个身份没认准就换了（多半是建新角色时那个临时的格子号）：它欠着的记录不写
            log(&format!("新装备系统：{} 没进到世界里就换成了格子 {}「{}」，它欠着的记录不写", st.who.as_ref().map_or("（没有）".to_string(), |w| format!("格子 {}「{}」", w.0, w.1)), who.0, who.1));
        }
        st.who = Some(who.clone());
        st.frames = 0;
        st.death = Default::default();
        on_load(d, st, &who, pgd);
        return;
    }
    texts_tick(d, st);
    // 死亡惩罚：放在「人物不在场就返回」前面（死亡画面后读图时也要接着看）
    deathloss::tick(d, st, pgd);
    // 人物不在场（主菜单、建新角色、读图）：这时背包里可能已经是别的角色——建新角色时格子号还是上一个存档的，名字也可能一样，
    // 认不出换了人（2026-11-01 用户：格子 7 的角色读着时回主菜单建了新角色，回购拿新角色的背包一比，把上一个角色的 132 件
    // 当成卖掉、挤掉，记录就丢了；开局装备也记错了格子）。不碰记录、不数背包，回来后重新数、不跟离开前的比；稳定帧数只数在场的
    if !unsafe { WorldChrMan::instance() }.ok().is_some_and(|w| w.main_player.is_some()) {
        st.last_held = None;
        return;
    }
    st.frames = st.frames.saturating_add(1);
    // 刚建的角色（who.rs 认准时游戏时间不到 3 分钟）：不接格子里原来的记录。要在补写前面（不然先把接过来的写回去）
    if let Some(g) = crate::who::fresh(&who).filter(|&g| st.fresh_gen != Some(g)) {
        st.fresh_gen = Some(g);
        start_fresh(d, st, &who, pgd);
    }
    if crate::who::is_confirmed(&who) && REC_OWED.swap(false, Ordering::Relaxed) {
        log(&format!("新装备系统：格子 {}「{}」已经进到世界里，补写记录", who.0, who.1));
        rec_save(&who, st);
    }
    deliver_check(d, st, pgd, &who);
    gift::tick(d, st, pgd, &who); // 联机装备赠送（gear/gift.rs）：先认刚到的赠送，再开新的一批；要在回购之前（送走的不进回购商店）
    if st.pending.is_none() && st.frames >= 60 {
        gift_tick(d, st, pgd);
    }
    buyback_tick(d, st, pgd, &who);
    shop_tick(d, st, pgd, &who);
    workshop::tick(d, st, pgd, &who);
    ashes::tick(d, st, pgd, &who); // 背包里的原版骨灰换成带稀有度的骨灰容器
    valfx::tick(d, st, pgd); // 词条数值随机：身上的随机词条挂写好精确数值的占位行
    if st.frames % 30 == 15 {
        workshop::gold_tick(d, st, pgd); // 超过原上限的补正在原版面板上变金色（scaling.rs）
    }
    // 装备回收请求（暗黑商店）：一次只该亮一个；看到就全部熄灭，进存档时残留的、不在赐福点的、同时亮了好几个的不回收
    if let Some(rc) = d.items.recycle {
        let lit: Vec<usize> = (0..4).filter(|&t| flag_get(rc.flags[t]) == Some(true)).collect();
        for &t in &lit {
            flag_set(rc.flags[t], false);
        }
        // 回收全部弃置（用户 2026-11-26）：和 4 档一样算「一个请求」，同时亮就别动
        let disc = rc.discard != 0 && flag_get(rc.discard) == Some(true);
        if disc {
            flag_set(rc.discard, false);
        }
        let n = lit.len() + disc as usize;
        if n == 0 {
            // 什么都没亮
        } else if st.frames < SETTLE_FRAMES {
            log("装备回收：进存档时回收请求亮着（存档里残留的），已熄灭、不回收");
        } else if n > 1 {
            log("装备回收：同时亮了不止一个回收请求，全部熄灭、不回收");
        } else if !at_grace() {
            log("装备回收：回收请求亮了，但人物不在赐福点，不回收");
        } else if disc {
            recycle_discarded(d, st, pgd, &who, rc);
        } else {
            recycle_batch(d, st, pgd, &who, rc, lit[0]);
        }
    }
    if st.frames % 30 == 0 {
        if let Some(w) = d.items.wish.as_ref() {
            if let Some(b) = player_block().and_then(|(a, b, r, i)| w.band_at(a, b, r, i)) {
                st.wish_band = Some(b);
            }
        }
        confirm_tick(d, st, pgd);
    }
    if st.frames % 60 == 30 {
        record_start(d, st, &who, pgd);
    }
    if st.frames >= SETTLE_FRAMES && st.frames % 60 == 45 {
        unsell_tick(st, Some(&who));
    }
    devgive::tick(d, st, pgd);
    // 许愿请求：看到就熄灭标记、记下次数（上一批还在发货就等它发完）。一次都付不起（没有免费次数、卢恩不够）就不记，
    // 点亮「诚意不够」再熄请求：赐福菜单等请求熄了，看这个标记提示「星辰没有收到你的诚意」（用户 2026-10-31）
    if let Some(w) = d.items.wish.as_ref() {
        for (flag, n) in [(w.flag1, 1), (w.flag10, 10)] {
            if flag_get(flag) == Some(true) {
                let cost = w.bands[wish_band(w, st)].cost;
                if st.frames < SETTLE_FRAMES {
                    log("向星辰许愿：进存档时许愿请求亮着（存档里残留的），已熄灭、不许愿");
                } else if !wish_affordable(wish_free_left(w), st.wish_queue, pgd.rune_count, cost) {
                    if w.poor != 0 {
                        flag_set(w.poor, true);
                    }
                    log(&format!("向星辰许愿：没有免费次数、卢恩不够（这个地区每次 {cost}，持有 {}），没许愿", pgd.rune_count));
                } else {
                    st.wish_queue += n;
                }
                flag_set(flag, false);
            }
        }
    }
    // 定向鉴定请求：看到就读种类、序号、稀有度，熄灭请求（菜单等它熄了再看「没鉴定」标记）；一件都鉴定不了点亮「没鉴定」
    if let Some(di) = d.items.dirid.as_ref() {
        if flag_get(di.flag) == Some(true) {
            let read = |(first, n): (u32, u32)| (0..n).filter(|&i| flag_get(first + i) == Some(true)).fold(0usize, |v, i| v | 1 << i);
            let (kind, k, rar) = (read(di.kind0), read(di.type0), read(di.rar0).min(5));
            let build = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
            let req = dir_req(d, di, kind, k, rar, build);
            let ok = if st.frames < SETTLE_FRAMES {
                log("定向鉴定：进存档时请求亮着（存档里残留的），已熄灭、不鉴定");
                true
            } else if !at_grace() {
                log("定向鉴定：请求亮了，但人物不在赐福点，不鉴定");
                false
            } else if let Some(req) = req {
                let cheapest = dir_min_cost(di, req.cat, &dir_unid(d, pgd, req.cat, rar));
                if cheapest.is_none_or(|c| pgd.rune_count < c) {
                    log(&format!("定向鉴定：{}，没鉴定", if cheapest.is_none() { format!("背包里没有这种稀有度的未鉴定{}", DIR_CAT_ZH[req.cat]) } else { "卢恩不够".to_string() }));
                    false
                } else if st.pending.is_some() || st.more || st.more_dir.is_some() || st.wish_queue > 0 {
                    log("定向鉴定：上一批还在发货，这次排在后面");
                    st.more_dir = Some(req);
                    true
                } else {
                    directed_batch(d, st, pgd, req)
                }
            } else {
                log(&format!("定向鉴定：种类 {kind}、序号 {k} 对不上（武器 {} 种、防具 {DIR_SLOTS} 个部位、护符 {} 个流派），没鉴定", di.types.len(), di.archs.len()));
                false
            };
            if !ok && di.none > 0 {
                flag_set(di.none, true);
            }
            flag_set(di.flag, false);
        }
    }
    if st.pending.is_some() || st.gift_wait.is_some() {
        return;
    }
    if let Some(req) = st.more_dir {
        directed_batch(d, st, pgd, req);
        return;
    }
    if st.wish_queue > 0 && !st.more {
        let n = st.wish_queue.min(d.items.deliver_rows);
        st.wish_queue -= n;
        wish_batch(d, st, pgd, n);
        return;
    }
    if st.more {
        identify_batch(d, st, pgd);
        return;
    }
    if flag_get(d.items.identify_flag) == Some(true) {
        flag_set(d.items.identify_flag, false);
        if st.frames < SETTLE_FRAMES {
            log("新装备系统：进存档时鉴定请求亮着（存档里残留的），已熄灭、不鉴定");
        } else if !at_grace() {
            log("新装备系统：鉴定请求亮了，但人物不在赐福点，不鉴定");
        } else {
            identify_batch(d, st, pgd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只进获得记录（斗技场的奖励）：按件数认，只认道具类；同一样东西别处照常发的不算；认完的从队列里去掉
    #[test]
    fn log_only_counts() {
        let mut want = vec![(9213, 2), (1290, 1), (400000, 1)];
        let batch = [(1, 9213, 1), (2, 400000, 1), (1, 9213, 3), (1, 1290, 5), (1, 9999, 1)];
        assert_eq!(take_log_only(&mut want, &batch), vec![(9213, 2), (1290, 1)]);
        assert_eq!(want, vec![(400000, 1)], "武器类的同号不算；道具类的认完就去掉");
        let mut v = Vec::new();
        add_count(&mut v, 7, 0);
        add_count(&mut v, 7, 2);
        add_count(&mut v, 7, 1);
        assert_eq!(v, vec![(7, 3)]);
    }

    /// 背包里的道具可以放在 normal_items_len 后面（2026-11-19 玩家诊断：融锻之心在第 708 格、len 655）。
    /// remove_goods 按 normal_entries（按容量）找到的格子，再用 normal_entries_mut 改数量：两边长度要一样，不然越界
    #[test]
    fn inventory_mut_entries_cover_capacity() {
        use eldenring::cs::InventoryItemsData;
        use std::mem::{ManuallyDrop, MaybeUninit};
        use std::ptr::addr_of_mut;
        const CAP: usize = 800;
        let mut entries = vec![0u64; CAP * 8]; // 每格按 64 字节放宽（实际不到）
        let mut other = [0u64; 8];
        let (head, dummy) = (entries.as_mut_ptr().cast::<u8>(), other.as_mut_ptr().cast::<u8>());
        let mut raw = MaybeUninit::<InventoryItemsData>::zeroed();
        let p = raw.as_mut_ptr();
        // 指针字段都写成非空（OwnedPtr / NonNull 不能是 0）；ManuallyDrop 不让它释放
        unsafe {
            addr_of_mut!((*p).normal_items_capacity).write(CAP as u32);
            addr_of_mut!((*p).normal_items_len).write(655);
            addr_of_mut!((*p).normal_items_head).cast::<*mut u8>().write(head);
            for f in [
                addr_of_mut!((*p).key_items_head).cast::<*mut u8>(),
                addr_of_mut!((*p).multiplay_key_items_head).cast::<*mut u8>(),
                addr_of_mut!((*p).normal_items_accessor.head).cast::<*mut u8>(),
                addr_of_mut!((*p).normal_items_accessor.length).cast::<*mut u8>(),
                addr_of_mut!((*p).key_items_accessor.head).cast::<*mut u8>(),
                addr_of_mut!((*p).key_items_accessor.length).cast::<*mut u8>(),
                addr_of_mut!((*p).item_id_mapping_indices).cast::<*mut u8>(),
                addr_of_mut!((*p).item_id_mapping).cast::<*mut u8>(),
            ] {
                f.write(dummy);
            }
        }
        let mut inv = ManuallyDrop::new(unsafe { raw.assume_init() });
        assert_eq!(inv.normal_entries().len(), CAP);
        assert_eq!(inv.normal_entries_mut().len(), CAP, "可改的格子也要按容量");
        assert!(inv.normal_entries_mut().get_mut(708).is_some());
    }

    /// 刚建的角色挪开旧记录：rec_aside_copy 不管名字都复制进 旧记录（没有文件时不复制）
    #[test]
    fn aside_copy_any_name() {
        let dir = std::env::temp_dir().join(format!("dr_rec_copy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("items_0.txt");
        assert!(rec_aside_copy(&p).is_none(), "没有文件");
        fs::write(&p, "name\tD\nw\t1\t0\t100\n").unwrap();
        let a = rec_aside_copy(&p).expect("同名也复制");
        let b = rec_aside_copy(&p).expect("同一秒再复制一份");
        assert_ne!(a, b);
        assert!(a.starts_with(dir.join(OLD_DIR)) && a.file_name().unwrap().to_string_lossy().starts_with("items_0_"));
        assert_eq!(fs::read_to_string(&a).unwrap(), "name\tD\nw\t1\t0\t100\n");
        assert_eq!(rec_find_aside(&p, "D").map(|x| x.1), Some("name\tD\nw\t1\t0\t100\n".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 角色记录：写别的角色前把旧的另存到 旧记录\，读档时按「同一格子 + 角色名」找回最新那份；格子 1 不会拿到格子 10 的
    #[test]
    fn record_set_aside_and_find() {
        let dir = std::env::temp_dir().join(format!("dr_rec_aside_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p0 = dir.join("items_1.txt");
        let head = "# 说明\n";
        // 没有文件、同名：都不另存
        assert!(!rec_set_aside(&p0, "bouu"));
        fs::write(&p0, format!("{head}name\tbouu\nw\t1\t0\t100\n")).unwrap();
        assert!(!rec_set_aside(&p0, "bouu"));
        assert_eq!(rec_text_name(&fs::read_to_string(&p0).unwrap()).as_deref(), Some("bouu"));
        // 换成别的角色：先另存
        assert!(rec_set_aside(&p0, "aabs"));
        fs::write(&p0, format!("{head}name\taabs\nw\t2\t0\t200\n")).unwrap();
        let (_, text) = rec_find_aside(&p0, "bouu").expect("找得回 bouu");
        assert!(text.contains("w\t1\t0\t100"));
        assert!(rec_find_aside(&p0, "nobody").is_none());
        // 再换回 bouu：aabs 那份也另存，两份都在
        assert!(rec_set_aside(&p0, "bouu"));
        assert!(rec_find_aside(&p0, "aabs").is_some());
        // 格子 10 的旧记录不算格子 1 的
        let p10 = dir.join("items_10.txt");
        fs::write(&p10, format!("{head}name\tzed\n")).unwrap();
        assert!(rec_set_aside(&p10, "other"));
        assert!(rec_find_aside(&p0, "zed").is_none());
        assert!(rec_find_aside(&p10, "zed").is_some());
        // 骨灰的记录没有说明行，第一行就是名字
        assert_eq!(rec_text_name("name\tx\n1\t2\n").as_deref(), Some("x"));
        assert_eq!(rec_text_name("\u{feff}name\ty\r\n").as_deref(), Some("y"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 继承换词条：被换掉的那条没有别的装备提供、身上挂着才摘；别的装备也有的留着；新的身上没有才挂
    #[test]
    fn worn_sync_delta() {
        let still = HashSet::from([20, 30, 99]);       // 改写后身上装备提供的（含目标的新效果 30）
        let have = HashSet::from([10, 11, 20, 99]);    // 身上现在挂着的
        let (gone, put) = worn_delta(&[10, 11, 20], &[20, 30, 11], &still, &have);
        assert_eq!(gone, vec![10], "10 换掉了、没人提供：摘；11 / 20 还在这件上：留");
        assert_eq!(put, vec![30], "30 新加、身上没有：挂");
        let (gone, _) = worn_delta(&[99], &[30], &still, &have);
        assert!(gone.is_empty(), "99 别的装备也有：留着");
        let (gone, put) = worn_delta(&[12], &[30], &still, &HashSet::from([30]));
        assert!(gone.is_empty() && put.is_empty(), "身上本来就没有 12、已经有 30：都不动");
        let (_, put) = worn_delta(&[-1], &[-1, 0], &still, &have);
        assert!(put.is_empty(), "空格不算");
    }

    /// 重随生成的 items.txt / affixes.txt：默认 dll\dark_core 里的；环境变量 DR_DEFS_DIR 指向别的目录（比如副本重随的输出）
    pub(super) fn real_defs() -> Option<(Items, Lib)> {
        let base = std::env::var_os("DR_DEFS_DIR").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dll").join("dark_core")
        });
        let it = parse_items(&fs::read_to_string(base.join("items.txt")).ok()?)?;
        let lib = parse_lib(&fs::read_to_string(base.join("affixes.txt")).ok()?).expect("affixes.txt 读不了");
        Some((it, lib))
    }

    /// 专属战技的行为变体：别的本体用「只有战技判定」的变体，原武器自己照旧，旧配方读档时换过来，没有 skillvar 的不动
    #[test]
    fn skill_var_mapping() {
        let mut lib = Lib::default();
        lib.skillvar.insert(1033, (403, 499));
        assert_eq!(skill_var(&lib, 405, 1033, 403), 499); // 碎星大剑 + 遗迹大剑的战技（旧配方存的 403）
        assert_eq!(skill_var(&lib, 403, 1033, 403), 403); // 遗迹大剑本体
        assert_eq!(skill_var(&lib, 403, 1033, 499), 403);
        assert_eq!(skill_var(&lib, 405, 1032, 405), 405); // 没有 skillvar 的战技
        assert_eq!(skill_var(&lib, 405, 1033, 405), 499); // 配方记的是本体的（当成战灰时存的）也换
        assert_eq!(skill_var(&lib, 405, 1033, 498), 499); // 以前分的 skillvar 编号（编号变了）
    }

    /// 判定换到共享层的专属战技（skillinfo 写成战灰）：不管配方里记的是什么变体，都用本体自己的；没换的照 skillvar
    #[test]
    fn row_var_uses_body_for_shared_skills() {
        let mut lib = Lib::default();
        lib.skillvar.insert(1033, (403, 499));
        lib.skillvar.insert(1012, (315, 398));
        lib.skillinfo.insert(1033, SkillInfo { gem: true, native: 403, name: Default::default() });
        lib.skillinfo.insert(1012, SkillInfo { gem: false, native: 315, name: Default::default() });
        assert_eq!(row_var(&lib, 405, 1033, 403), 405); // 旧配方存的原武器变体 → 本体的
        assert_eq!(row_var(&lib, 405, 1033, 499), 405); // 旧的 skillvar 变体 → 本体的
        assert_eq!(row_var(&lib, 403, 1033, 403), 403);
        assert_eq!(row_var(&lib, 302, 1012, 315), 398); // 欧赫剑舞（没换号）照旧
        assert_eq!(row_var(&lib, 302, 1012, 399), 398);
        assert_eq!(row_var(&lib, 302, -1, 302), 302); // 没战技（成品传说照本体）
    }

    /// 许愿付不付得起：免费次数先用（排着的也算），再按价格
    #[test]
    fn wish_affordable_counts_queue() {
        assert!(wish_affordable(1, 0, 0, 3000)); // 还有免费
        assert!(!wish_affordable(0, 0, 2999, 3000));
        assert!(wish_affordable(0, 0, 3000, 3000));
        assert!(!wish_affordable(1, 1, 2999, 3000)); // 免费那次给了排着的
        assert!(wish_affordable(0, 2, 9000, 3000));
        assert!(!wish_affordable(0, 2, 8999, 3000));
        // items.txt 的 wish 行：第 9 列是「诚意不够」标记，旧的没有就是 0
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dll").join("dark_core");
        let Ok(text) = fs::read_to_string(base.join("items.txt")) else { return };
        let text: String = text.lines().filter(|l| !l.starts_with("wish\t")).map(|l| format!("{l}\n")).collect();
        let it = |extra: &str| parse_items(&format!("{text}wish\t93503\t93504\t93020\t30\t30\t60\t10{extra}\n")).expect("items.txt 读不了");
        assert_eq!(it("").wish.map(|w| (w.flag1, w.poor)), Some((93503, 0)));
        assert_eq!(it("\t93909").wish.map(|w| w.poor), Some(93909));
    }

    /// 新词条饰品图标：famicon 是虚编号（45000~49999）、frames.txt 里有带框的才换；原版图标的词条照旧走 frame_row；
    /// 真的词条库后面加 famicon 行能读出来
    #[test]
    fn affix_art_icon() {
        let mut lib = Lib::default();
        lib.famicon.insert("dr_x".into(), [45003; 4]);
        lib.famicon.insert("old".into(), [18800; 4]);
        lib.ficon.insert((45003, 2), 57014);
        lib.ficon.insert((18800, 2), 50010);
        assert_eq!(art_icon(&lib, "dr_x", 2), Some(57014));
        assert_eq!(art_icon(&lib, "dr_x", 1), None); // 没有带框的（菜单包没合成）
        assert_eq!(art_icon(&lib, "old", 2), None);
        assert_eq!(art_icon(&lib, "nope", 2), None);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dll").join("dark_core");
        let Ok(text) = fs::read_to_string(base.join("affixes.txt")) else { return };
        let text: String = text.lines().filter(|l| !l.starts_with("famicon\tdr_x\t")).map(|l| format!("{l}\n")).collect();
        let lib = parse_lib(&format!("{text}famicon\tdr_x\t0,45001,45001,45001\n")).unwrap();
        assert_eq!(lib.famicon.get("dr_x"), Some(&[0, 45001, 45001, 45001]));
    }

    /// 重武器那行（wheavy）：真的词条库后面加一行能读出来；倍率不对报错
    #[test]
    fn wheavy_line() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dll").join("dark_core");
        let Ok(text) = fs::read_to_string(base.join("affixes.txt")) else { return };
        let text: String = text.lines().filter(|l| !l.starts_with("wheavy\t")).map(|l| format!("{l}\n")).collect();
        let lib = parse_lib(&format!("{text}wheavy\t1.5\t5,7,41\n")).unwrap();
        assert_eq!(lib.wheavy, HashMap::from([(5, 1.5), (7, 1.5), (41, 1.5)]));
        assert!(parse_lib(&format!("{text}wheavy\t0\t5\n")).is_err());
    }

    /// 按武器类型的补正曲线（wcurvet，用户 2026-11-25「照原版重做」）：一行一组伤害种类共用一次抽到的曲线；
    /// 写到的种类照它，没写到的、没有 wcurvet 的类型照旧按稀有度的 wcurve；种类越界报错
    #[test]
    fn curves_by_type() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dll").join("dark_core");
        let Ok(text) = fs::read_to_string(base.join("affixes.txt")) else { return };
        let mut text: String = text.lines().filter(|l| !l.starts_with("wcurvet\t")).map(|l| format!("{l}\n")).collect();
        let bad = text.clone();
        for k in 0..CURVE_N {
            text += &format!("wcurve\t3\t{k}\t9:1\n");
        }
        text += "wcurvet\t3\t0\t0:7\t1:1\t2:1\t8:1\t7:1\nwcurvet\t3\t1,2,3,4\t4:1\nwcurvet\t3\t5,6,7,8\t6:1\n";
        text += "wcurvet\t57\t0,1,2,3,4\t0:9\t16:8\t15:8\t12:3\t14:1\n";
        let lib = parse_lib(&text).unwrap();
        let mut rng = Rng(12345);
        let mut phys = HashMap::new();
        for _ in 0..2000 {
            let c = draw_curves(&lib, 3, 3, &mut rng);
            assert_eq!(&c[1..5], &[4; 4], "元素一律 4 号");
            assert_eq!(&c[5..], &[6; 4], "异常 6 号");
            *phys.entry(c[0]).or_insert(0) += 1;
            let s = draw_curves(&lib, 3, 57, &mut rng);
            assert!(s[..5].iter().all(|&x| x == s[0]) && [0, 16, 15, 12, 14].contains(&s[0]), "法杖五种伤害同一条：{s:?}");
            assert_eq!(&s[5..], &[9; 4], "法杖没写异常：照稀有度的 wcurve");
        }
        assert!(phys.keys().all(|k| [0, 1, 2, 7, 8].contains(k)));
        assert!(phys[&0] > 1100 && phys[&0] < 1700, "0 号约七成：{phys:?}");
        assert_eq!(draw_curves(&lib, 3, 42, &mut rng), [9; CURVE_N], "没有 wcurvet 的类型照旧");
        assert!(parse_lib(&format!("{bad}wcurvet\t3\t9\t4:1\n")).is_err(), "种类越界");
        assert!(parse_lib(&format!("{bad}wcurvet\t3\t\t4:1\n")).is_err(), "没写种类");
        assert!(parse_lib(&format!("{bad}wcurvet\t3\t0\t4:1\n")).is_ok());
    }

    /// 指定了 DR_DEFS_DIR（带 wcurvet 的新重随输出）时：每档生成一批武器，元素曲线都是 4 号、异常 6 号，法杖 / 圣印记五种伤害同一条、不是 0 号
    #[test]
    fn generated_curves_follow_vanilla() {
        if std::env::var_os("DR_DEFS_DIR").is_none() {
            eprintln!("没设 DR_DEFS_DIR，跳过");
            return;
        }
        let (_items, lib) = real_defs().expect("DR_DEFS_DIR 读不了 items.txt / affixes.txt");
        assert!(!lib.wcurvet.is_empty(), "定义里没有 wcurvet 行");
        let mut rng = Rng(777);
        let (mut n, mut phys) = (0, HashMap::new());
        for tier in 0..5 {
            for _ in 0..600 {
                let Some(Recipe::W { base, curve, .. }) = gen_weapon(&lib, tier, &mut rng) else { continue };
                let typ = lib.wbase.iter().find(|b| b.id == base).map(|b| b.typ).unwrap();
                n += 1;
                assert_eq!(&curve[5..], &[6; 4], "异常 6 号：{base} {curve:?}");
                if typ == 57 || typ == 61 {
                    assert!(curve[..5].iter().all(|&c| c == curve[0]) && curve[0] != 0, "法器同一条、不是 0 号：{base} {curve:?}");
                } else {
                    assert_eq!(&curve[1..5], &[4; 4], "元素 4 号：{base} {curve:?}");
                    *phys.entry(curve[0]).or_insert(0u32) += 1;
                }
            }
        }
        eprintln!("生成 {n} 把，物理曲线 {phys:?}");
        assert!(n > 2000);
    }

    /// famtop（同名只认最好的那件）：指定了 DR_DEFS_DIR（新的重随输出）时，蓄力攻击伤害、双手持握伤害都在里面，而且是存在的词条
    #[test]
    fn top_only_from_real_defs() {
        if std::env::var_os("DR_DEFS_DIR").is_none() {
            eprintln!("没设 DR_DEFS_DIR，跳过");
            return;
        }
        let (_items, lib) = real_defs().expect("DR_DEFS_DIR 读不了 items.txt / affixes.txt");
        for k in ["increase_charged_attacks_by_pct", "increase_damage_with_two_handed_weapons_by_pct"] {
            assert!(lib.top_only.contains(k) && lib.fam_by_key.contains_key(k), "{k} 不在 famtop 里：{:?}", lib.top_only);
        }
        assert_eq!(lib.top_only.len(), 2, "{:?}", lib.top_only);
    }

    /// 强化映射：每种原版强化类型四个稀有度 × 全能都有、编号不重、材料组都有
    #[test]
    fn upgrade_map_from_real_defs() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 affixes.txt（先跑一次重随），跳过");
            return;
        };
        if lib.rmax == 0 {
            eprintln!("这次重随没开 weapon_upgrade，跳过");
            return;
        }
        let types: HashSet<u16> = lib.reinf.keys().map(|k| k.0).collect();
        assert_eq!(lib.reinf.len(), types.len() * 8, "每种强化类型要有 4 个稀有度 × 全能");
        let ids: HashSet<i32> = lib.reinf.values().copied().collect();
        if lib.womni.is_some() {
            // 2026-11-24 起全能和其他武器同一个强化类型（全能只靠 womni 打折）
            assert!(lib.reinf.iter().all(|(&(t, r, om), v)| !om || lib.reinf.get(&(t, r, false)) == Some(v)), "全能要和其他武器同一个强化类型");
            assert_eq!(ids.len(), types.len() * 4, "换成的强化类型有重复");
        } else {
            assert_eq!(ids.len(), lib.reinf.len(), "换成的强化类型有重复");
        }
        assert!(ids.iter().all(|&t| t % 100 == 0 && t + lib.rmax as i32 <= i16::MAX as i32), "强化类型编号：{ids:?}");
        assert!(lib.rmat.iter().all(|&m| m > 0), "材料组：{:?}", lib.rmat);
        eprintln!("强化：{} 种原版强化类型，最高 +{}", types.len(), lib.rmax);
    }

    #[test]
    fn affix_weights_by_rarity() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 affixes.txt（先跑一次重随），跳过");
            return;
        };
        // 按稀有度分开的词条族（famw）：权重是 0 的稀有度一次都不出；四档全 0 的（停用，留给已有装备）哪里都不出
        let split: Vec<&Fam> = lib.fams.iter().filter(|f| f.tal_w.iter().any(|&w| w == 0) && f.tal_w.iter().any(|&w| w > 0)).collect();
        let off: Vec<&str> = lib.fams.iter().filter(|f| f.armor_w == [0; 4] && f.tal_w == [0; 4]).map(|f| f.key.as_str()).collect();
        if split.is_empty() && off.is_empty() {
            eprintln!("affixes.txt 里没有按稀有度的权重（旧格式），跳过");
            return;
        }
        let mut rng = Rng(5);
        for r in 0..4 {
            let mut seen: HashSet<String> = HashSet::new();
            for _ in 0..30000 {
                if let Some(Recipe::A { key, .. }) = gen_talisman(&lib, r, &mut rng) {
                    seen.insert(key);
                }
            }
            for f in &split {
                assert_eq!(seen.contains(&f.key), f.tal_w[r] > 0, "{} 在稀有度 {r} 的护符权重是 {}", f.key, f.tal_w[r]);
            }
            for k in &off {
                assert!(!seen.contains(*k), "停用的 {k} 在稀有度 {r} 出了");
            }
        }
        eprintln!("按稀有度分开的 {} 个、停用的 {:?}", split.len(), off);
    }

    #[test]
    fn dist_and_choose() {
        let d = Dist::parse(&["1:0", "2:3", "5:1"]).unwrap();
        assert_eq!(d.total, 4);
        let mut rng = Rng(7);
        let mut c = HashMap::new();
        for _ in 0..4000 {
            *c.entry(d.pick(&mut rng).unwrap() as i32).or_insert(0) += 1;
        }
        assert!(!c.contains_key(&1) && c[&2] > 2700 && c[&5] > 800, "{c:?}");
        let v = choose(&mut rng, 5, 3);
        assert_eq!(v.len(), 3);
        assert_eq!(v.iter().collect::<HashSet<_>>().len(), 3);
        let w = choose_weighted(&mut rng, &[0, 5, 5, 0], 3);
        assert_eq!(w.iter().copied().collect::<HashSet<_>>(), HashSet::from([1, 2]));
        assert!(Dist::parse(&["x"]).is_none());
        let t = Dist::parse(&["10:5", "20:3", "30:2"]).unwrap().top(0.3);
        assert_eq!(t.items.iter().map(|x| x.0 as i32).collect::<Vec<_>>(), vec![30, 20], "前 30%：30（2）不够 3，带上 20");
        assert_eq!(Dist::parse(&["10:5"]).unwrap().top(1.0).total, 5);
    }

    #[test]
    fn records_roundtrip() {
        let w = Rec {
            cat: 0,
            idx: 12,
            tier: 3,
            recipe: Recipe::W { base: 1000000, var: 100, skill: 10, cor: [45.5, 0.0, 30.0, 0.0, 0.0], req: [12, 0, 15, 0, 0], curve: [0, 1, 2, 3, 4, 5, 6, 7, 8], st: [6410, -1, -1], res: [-1, -1], glow: 7092020, spec: 7210391, atk: 15, ovl: false, elem: 0, share: 0 },
        };
        let p = Rec { cat: 1, idx: 5, tier: 4, recipe: Recipe::P { base: 40000, sps: vec![(7002510, "max_fp_pct".into()), (7200003, "sorcery_dmg".into())], mult: None, pos: Vec::new() } };
        let a = Rec { cat: 2, idx: 0, tier: 1, recipe: Recipe::A { tal: 9001, sp: 7200001, key: "sorcery_dmg".into(), pos: 0 } };
        // 开局装备记进角色记录：序号带 START_BIT，按编号段换算
        let s = Rec { cat: 1, idx: START_BIT | 7, tier: 2, recipe: Recipe::P { base: 40000, sps: vec![(7200003, "sorcery_dmg".into())], mult: None, pos: Vec::new() } };
        for r in [&w, &p, &a, &s] {
            assert_eq!(rec_parse(&rec_line(r)).as_ref(), Some(r), "{}", rec_line(r));
        }
        // 加强攻之前的记录少最后一列：当没有强攻
        let old = rec_line(&w).rsplit_once('\t').unwrap().0.to_string();
        assert!(matches!(rec_parse(&old).unwrap().recipe, Recipe::W { atk: 0, spec: 7210391, .. }));
        let it = Items { start_ids: [(230000000, 10000), (29000000, 100)], ..Default::default() };
        assert_eq!(rec_id(&it, &s), 29000700);
        assert_eq!(start_off(&it, 1, 29000700), Some(s.idx));
        assert_eq!(start_off(&it, 0, 230030000), Some(START_BIT | 3));
        assert_eq!(start_off(&it, 0, 230030050), None, "不是整万");
        assert_eq!(start_off(&it, 0, 1000000), None);
        let recs: HashMap<(usize, u32), Rec> = [w.clone(), p, s].into_iter().map(|r| ((r.cat, r.idx), r)).collect();
        // 回购商店里的按格子顺序存（空格不存）
        // 武器商店正在卖的另外存
        let shop = vec![Rec { idx: 14, ..w.clone() }];
        let sold = vec![Some(a.clone()), None, Some(Rec { idx: 13, ..w })];
        let t = rec_text("褪色者", &recs, &sold, &shop);
        let (r2, s2, h2) = rec_load(&t, "褪色者").unwrap();
        assert_eq!(r2, recs);
        assert_eq!(s2, sold.iter().flatten().cloned().collect::<Vec<_>>());
        assert_eq!(h2, shop);
        assert!(rec_load(&t, "别人").is_none());
        // 旧版 DLL 读到 sold / shop 行：不认识，跳过
        assert!(t.lines().filter(|l| l.starts_with("sold\t") || l.starts_with("shop\t")).all(|l| rec_parse(l).is_none()));
    }

    #[test]
    fn armor_positions_by_type() {
        let mut lib = Lib::default();
        // a、d 机制类；b、c 数值类同一个合并组，e 数值类
        for (key, mech, group) in [("a", true, ""), ("b", false, "g"), ("c", false, "g"), ("d", true, ""), ("e", false, "")] {
            lib.fams.push(Fam {
                key: key.into(),
                armor_w: [0; 4],
                tal_w: [0; 4],
                suffix: [key.into(), key.into()],
                r: [None, None, None, None],
                slots: 15,
                mech,
                group: group.into(),
            });
        }
        let w = |f: &Fam| if f.group.is_empty() { 1200 } else { 600 };
        let mut rng = Rng(7);
        for _ in 0..500 {
            // 三个位置都有：前缀一个机制类在前，两个数值类，同组只出一个（所以一定有 e）
            let v = armor_positions(&lib, &[100, 100, 100], w, &mut rng);
            assert_eq!(v.len(), 3);
            assert!(lib.fams[v[0]].mech && !lib.fams[v[1]].mech && !lib.fams[v[2]].mech, "{v:?}");
            assert!(v.contains(&4), "同组的 b、c 不能一起出：{v:?}");
            // 都没掷中：补一个后缀
            let v = armor_positions(&lib, &[0, 0, 0], w, &mut rng);
            assert!(v.len() == 1 && !lib.fams[v[0]].mech);
        }
        // 几率：前缀 25%、后缀 2 40%
        let n = 20000;
        let (mut pre, mut two) = (0, 0);
        for _ in 0..n {
            let v = armor_positions(&lib, &[25, 100, 40], w, &mut rng);
            pre += lib.fams[v[0]].mech as usize;
            two += (v.iter().filter(|&&k| !lib.fams[k].mech).count() == 2) as usize;
        }
        let (pre, two) = (pre as f64 / n as f64, two as f64 / n as f64);
        assert!((pre - 0.25).abs() < 0.015 && (two - 0.40).abs() < 0.015, "前缀 {pre:.3}、两个后缀 {two:.3}");
        // 位置上每条一样：e 的权重是 b、c 的两倍（b、c 是一组），出现次数 e ≈ b + c
        let (mut be, mut cnt_e) = (0, 0);
        for _ in 0..n {
            let v = armor_positions(&lib, &[0, 100, 0], w, &mut rng);
            if v[0] == 4 { cnt_e += 1 } else { be += 1 }
        }
        assert!((cnt_e as f64 / n as f64 - 0.5).abs() < 0.02, "e {cnt_e} / 组 {be}");
    }

    #[test]
    fn names_prefix_and_suffix() {
        let s2 = |a: &str, b: &str| [a.to_string(), b.to_string()];
        let mut lib = Lib::default();
        lib.rar = [s2("普通", "Common"), s2("稀有", "Rare"), s2("史诗", "Epic"), s2("传说", "Legendary")];
        lib.stat = vec![s2("力气", "Str"), s2("灵巧", "Dex"), s2("智力", "Int"), s2("信仰", "Fai"), s2("感应", "Arc")];
        lib.pbase.push(PBase { slot: 1, id: 40000, name: s2("骑士铠甲", "Knight Armor") });
        lib.wbase.push(WBase { typ: 3, id: 1000000, var: 100, elem: 0, name: s2("短剑", "Dagger") });
        let fams = [("a", "壁垒", "Bulwark", true), ("b", "力气", "Strength", false), ("c", "生命力", "Vitality", false), ("d", "反震", "Recoil", true), ("e", "魔力", "Magic", false)];
        for (k, (key, zh, en, mech)) in fams.into_iter().enumerate() {
            lib.fam_by_key.insert(key.into(), k);
            lib.fams.push(Fam { key: key.into(), armor_w: [0; 4], tal_w: [0; 4], suffix: s2(zh, en), r: [None, None, None, None], slots: 15, mech, group: String::new() });
        }
        lib.wspec.push(WSpec { key: "dr_giant".into(), sp: 7, w: 1, types: vec![], name: s2("巨人杀手", "Giant Slayer"), text: s2("…", "…"), rar: 15 });
        let p = |keys: &[&str]| Rec { cat: 1, idx: 0, tier: 3, recipe: Recipe::P { base: 40000, sps: keys.iter().map(|k| (1, k.to_string())).collect(), mult: None, pos: Vec::new() } };
        let w = |cor: [f32; 5], spec: i32| Rec {
            cat: 0,
            idx: 0,
            tier: 3,
            recipe: Recipe::W { base: 1000000, var: 100, skill: -1, cor, req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        let name = |rec: &Rec, lang: usize| compose_cap(&lib, rec, lang, 160)[0].clone();
        // 防具：第一个机制类做前缀，数值类最多两个做后缀
        assert_eq!(name(&p(&["b", "a", "c"]), 0), "传说 壁垒 骑士铠甲·力气·生命力");
        assert_eq!(name(&p(&["b", "a", "c"]), 1), "Legendary Bulwark Knight Armor of Strength and Vitality");
        assert_eq!(name(&p(&["a", "d"]), 0), "传说 壁垒 骑士铠甲", "第二个机制类不进名字");
        assert_eq!(name(&p(&["b", "c", "e"]), 0), "传说 骑士铠甲·力气·生命力", "后缀最多两个");
        assert_eq!(name(&p(&["e"]), 1), "Legendary Knight Armor of Magic");
        // 武器：只加前缀（全能在特效前面）
        assert_eq!(name(&w([50.0, 0.0, 0.0, 0.0, 0.0], 7), 0), "传说 巨人杀手 短剑");
        assert_eq!(name(&w([50.0; 5], 7), 0), "传说 全能·巨人杀手 短剑");
        assert_eq!(name(&w([50.0; 5], 7), 1), "Legendary Versatile Giant Slayer Dagger");
        assert_eq!(name(&w([50.0; 5], -1), 0), "传说 全能 短剑");
        assert_eq!(name(&w([50.0, 0.0, 0.0, 0.0, 0.0], -1), 0), "传说 短剑");
        // 原包有的英文后缀全小写：名字里每个词首字母大写
        lib.fams[2].suffix[1] = "long forgotten scales".into();
        assert_eq!(compose_cap(&lib, &p(&["a", "c"]), 1, 160)[0], "Legendary Bulwark Knight Armor of Long Forgotten Scales");
    }

    /// 数值模型用的装备样本（平时不跑，dev\power_curve\dr_pool.py 调）：
    /// DR_DEFS_DIR=<定义目录> DR_POOL_BASE=<本体行> DR_POOL_OUT=<输出> [DR_POOL_N=每类每档件数] cargo test --release export_pool -- --ignored --nocapture
    /// 本体行一行「w / p<TAB>编号<TAB>整行十六进制」（从重随输出的 regulation 里抽）；
    /// 输出一行「配方（rec_line）<TAB>生成后的整行十六进制」（护符没有行，留空），第一行「bias<TAB>鉴定偏向加点的候选数」。
    /// 配方和整行都是游戏里鉴定时同一套代码（generate + build_row）出的，模型照着算攻击力、减伤、词条
    #[test]
    #[ignore]
    fn export_pool() {
        let (_items, lib) = real_defs().expect("DR_DEFS_DIR 没设或读不了 items.txt / affixes.txt");
        let base_text = fs::read_to_string(std::env::var("DR_POOL_BASE").expect("要设 DR_POOL_BASE")).expect("本体行读不了");
        let out_path = std::env::var("DR_POOL_OUT").expect("要设 DR_POOL_OUT");
        let n: u32 = std::env::var("DR_POOL_N").ok().and_then(|x| x.parse().ok()).unwrap_or(400);
        let mut base: HashMap<(usize, u32), Vec<u8>> = HashMap::new();
        for f in base_text.lines().map(|l| l.split('\t').collect::<Vec<_>>()).filter(|f| f.len() >= 3) {
            let cat = match f[0] {
                "w" => 0,
                "p" => 1,
                _ => continue,
            };
            let row = (0..f[2].len() / 2).map(|i| u8::from_str_radix(&f[2][2 * i..2 * i + 2], 16).expect("十六进制不对")).collect();
            base.insert((cat, f[1].parse().expect("编号不对")), row);
        }
        let mut rng = Rng(20261030);
        let mut out = format!("bias\t{}\n", lib.bias_n);
        let (mut made, mut missing) = (0, 0);
        for cat in 0..NCAT {
            for tier in 0..5 {
                for idx in 0..n {
                    let Some(recipe) = generate(&lib, cat, tier, &mut rng) else { continue };
                    let rec = Rec { cat, idx, tier: tier as u8, recipe };
                    let hex = match &rec.recipe {
                        Recipe::W { base: b, .. } | Recipe::P { base: b, .. } => match base.get(&(cat, *b)) {
                            Some(br) => build_row(&lib, &rec, br, false).iter().map(|x| format!("{x:02x}")).collect(),
                            None => {
                                missing += 1;
                                continue;
                            }
                        },
                        Recipe::A { .. } => String::new(),
                    };
                    out.push_str(&format!("{}\t{hex}\n", rec_line(&rec)));
                    made += 1;
                }
            }
        }
        fs::write(&out_path, out).expect("写不了输出");
        assert!(missing * 20 < made, "本体行缺得太多：缺 {missing}，生成 {made}");
        println!("装备样本 {made} 件（缺本体行跳过 {missing}）→ {out_path}");
    }

    /// 按存档算伤害成长用（平时不跑，dev\power_curve\save_curve.py 调）：角色记录（items_<格子>.txt）里的配方 → 游戏里那一行
    /// DR_DEFS_DIR=<定义目录> DR_POOL_BASE=<本体行，同 export_pool> DR_REC_IN=<一行「键<TAB>配方」> DR_REC_OUT=<输出> cargo test --release export_records -- --ignored --nocapture
    /// 输出一行「键<TAB>参数行编号<TAB>整行十六进制」（只有武器 / 防具；本体行缺的跳过）；
    /// 后面再跟开局装备（items.txt 的 start 行按种子生成，和 start_recs 一样；不在角色记录里的开局武器用这份），键是「start」
    #[test]
    #[ignore]
    fn export_records() {
        let (items, lib) = real_defs().expect("DR_DEFS_DIR 没设或读不了 items.txt / affixes.txt");
        let mut lines: Vec<(String, String)> = Vec::new();
        for s in items.start.iter().filter(|s| s.cat == 0) {
            let mut rng = Rng(s.seed);
            if let (Some(recipe), Some(idx)) = (gen_weapon_on(&lib, s.tier as usize, Some(s.wfix()), &mut rng), start_off(&items, s.cat, s.id)) {
                lines.push(("start".into(), rec_line(&Rec { cat: s.cat, idx, tier: s.tier, recipe })));
            }
        }
        let base_text = fs::read_to_string(std::env::var("DR_POOL_BASE").expect("要设 DR_POOL_BASE")).expect("本体行读不了");
        let in_text = fs::read_to_string(std::env::var("DR_REC_IN").expect("要设 DR_REC_IN")).expect("配方读不了");
        let out_path = std::env::var("DR_REC_OUT").expect("要设 DR_REC_OUT");
        let mut base: HashMap<(usize, u32), Vec<u8>> = HashMap::new();
        for f in base_text.lines().map(|l| l.split('\t').collect::<Vec<_>>()).filter(|f| f.len() >= 3) {
            let cat = match f[0] {
                "w" => 0,
                "p" => 1,
                _ => continue,
            };
            let row = (0..f[2].len() / 2).map(|i| u8::from_str_radix(&f[2][2 * i..2 * i + 2], 16).expect("十六进制不对")).collect();
            base.insert((cat, f[1].parse().expect("编号不对")), row);
        }
        let (mut out, mut made, mut missing) = (String::new(), 0, 0);
        let input = in_text.lines().filter_map(|l| l.split_once('\t')).map(|(k, l)| (k.to_string(), l.to_string()));
        for (key, line) in input.collect::<Vec<_>>().into_iter().chain(lines) {
            let Some(rec) = rec_parse(&line) else { continue };
            let b = match &rec.recipe {
                Recipe::W { base: b, .. } | Recipe::P { base: b, .. } => *b,
                Recipe::A { .. } => continue,
            };
            let Some(br) = base.get(&(rec.cat, b)) else {
                missing += 1;
                continue;
            };
            let hex: String = build_row(&lib, &rec, br, false).iter().map(|x| format!("{x:02x}")).collect();
            out.push_str(&format!("{key}\t{}\t{hex}\n", rec_id(&items, &rec)));
            made += 1;
        }
        fs::write(&out_path, out).expect("写不了输出");
        println!("记录里的装备 {made} 件（缺本体行跳过 {missing}）→ {out_path}");
    }

    /// 防具 / 护符逐件样本（平时不跑；算「凑齐一套要鉴定多少件」这类要看同一件上词条组合的）：
    /// DR_DEFS_DIR=<定义目录> DR_SAMPLES_OUT=<输出> [DR_SAMPLES_N=每档件数，默认 50000] cargo test --release gear_samples -- --ignored --nocapture
    /// 输出一行一件：「p<TAB>档<TAB>部位 0~3<TAB>词条键,…」或「a<TAB>档<TAB>-<TAB>词条键」
    #[test]
    #[ignore]
    fn gear_samples() {
        let (_items, lib) = real_defs().expect("DR_DEFS_DIR 没设或读不了 items.txt / affixes.txt");
        let out_path = std::env::var("DR_SAMPLES_OUT").expect("要设 DR_SAMPLES_OUT");
        let n: u32 = std::env::var("DR_SAMPLES_N").ok().and_then(|x| x.parse().ok()).unwrap_or(50000);
        let mut rng = Rng(20261031);
        let mut out = String::new();
        for t in 0..5 {
            for _ in 0..n {
                if let Some(Recipe::P { base, sps, .. }) = gen_armor(&lib, t, &mut rng) {
                    let slot = lib.pbase.iter().find(|b| b.id == base).map_or(9, |b| b.slot);
                    out.push_str(&format!("p\t{t}\t{slot}\t{}\n", sps.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(",")));
                }
                if let Some(Recipe::A { key, .. }) = gen_talisman(&lib, t, &mut rng) {
                    out.push_str(&format!("a\t{t}\t-\t{key}\n"));
                }
            }
        }
        fs::write(&out_path, out).expect("写不了输出");
        println!("防具 / 护符样本：每档各 {n} 件 → {out_path}");
    }

    /// 抽取分布统计（平时不跑）：DR_DEFS_DIR=<定义目录> [DR_STATS_OUT=<计数文件>] cargo test --release draw_stats -- --ignored --nocapture
    /// 计数文件一行「种类<TAB>档<TAB>键<TAB>件数」（dev\item_system\draw_table.py 读它做表）
    #[test]
    #[ignore]
    fn draw_stats() {
        let Some((_items, lib)) = real_defs() else { return };
        const N: usize = 20000;
        const TIER: [&str; 5] = ["普通", "稀有", "史诗", "传说", "传说（幽影）"];
        let mech = |k: &str| lib.fam_by_key.get(k).is_some_and(|&i| lib.fams[i].mech);
        let pct = |a: usize| format!("{:.1}%", a as f64 * 100.0 / N as f64);
        let mut rng = Rng(12345);
        let mut out = format!("n\t-\t-\t{N}\n");
        let mut dump = |kind: &str, t: usize, key: &str, n: usize| out.push_str(&format!("{kind}\t{t}\t{key}\t{n}\n"));
        for t in 0..5 {
            // 防具：词条数、有没有前缀、名字里几个后缀、没进名字的词条、机制类占词条的比例
            let (mut cnt, mut pre, mut post, mut hidden, mut n_mech, mut n_all) = ([0usize; 4], 0, [0usize; 3], 0, 0, 0);
            let mut top: HashMap<String, usize> = HashMap::new();
            let (mut aff, mut first): (HashMap<String, usize>, HashMap<String, usize>) = Default::default();
            for _ in 0..N {
                let Some(Recipe::P { sps, .. }) = gen_armor(&lib, t, &mut rng) else { continue };
                let m = sps.iter().filter(|x| mech(&x.1)).count();
                let v = sps.len() - m;
                cnt[sps.len()] += 1;
                pre += (m > 0) as usize;
                post[v.min(2)] += 1;
                hidden += (m > 1 || v > 2) as usize;
                n_mech += m;
                n_all += sps.len();
                for x in &sps {
                    *aff.entry(x.1.clone()).or_default() += 1;
                }
                if let Some(x) = sps.iter().find(|x| mech(&x.1)) {
                    *top.entry(suffix(&lib, &x.1, 0)).or_default() += 1;
                    *first.entry(x.1.clone()).or_default() += 1;
                }
            }
            for k in 1..=3 {
                dump("armor_cnt", t, &k.to_string(), cnt[k]);
            }
            for k in 0..3 {
                dump("armor_post", t, &k.to_string(), post[k]);
            }
            dump("armor_pre", t, "-", pre);
            dump("armor_hidden", t, "-", hidden);
            dump("armor_mech", t, "-", n_mech);
            dump("armor_all", t, "-", n_all);
            for (k, n) in &aff {
                dump("armor_aff", t, k, *n);
            }
            for (k, n) in &first {
                dump("armor_prefix", t, k, *n);
            }
            let mut top: Vec<_> = top.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1));
            eprintln!(
                "【防具 {}】词条 1/2/3 个：{} / {} / {}；有前缀 {}；后缀 0/1/2 个：{} / {} / {}；有词条没进名字 {}；机制类占全部词条 {:.1}%；前缀最常见：{}",
                TIER[t], pct(cnt[1]), pct(cnt[2]), pct(cnt[3]), pct(pre), pct(post[0]), pct(post[1]), pct(post[2]), pct(hidden),
                n_mech as f64 * 100.0 / n_all.max(1) as f64,
                top.iter().take(5).map(|(k, n)| format!("{k} {}", pct(*n))).collect::<Vec<_>>().join("、")
            );
            // 武器：特效（前缀）、全能、强攻
            let (mut sp, mut full, mut atk) = (0, 0, 0);
            let mut specs: HashMap<String, usize> = HashMap::new();
            for _ in 0..N {
                let Some(Recipe::W { spec, cor, atk: a, .. }) = gen_weapon(&lib, t, &mut rng) else { continue };
                sp += (spec > 0) as usize;
                full += (cor.iter().filter(|&&c| c > 0.0).count() >= 5) as usize;
                atk += (a > 0) as usize;
                if let Some(x) = lib.wspec.iter().find(|x| x.sp == spec) {
                    *specs.entry(x.key.clone()).or_default() += 1;
                }
            }
            dump("weapon_spec_any", t, "-", sp);
            dump("weapon_full", t, "-", full);
            dump("weapon_atk", t, "-", atk);
            for (k, n) in &specs {
                dump("weapon_spec", t, k, *n);
            }
            // 护符：机制类占比
            let mut tm = 0;
            let mut tals: HashMap<String, usize> = HashMap::new();
            for _ in 0..N {
                if let Some(Recipe::A { key, .. }) = gen_talisman(&lib, t, &mut rng) {
                    tm += mech(&key) as usize;
                    *tals.entry(key).or_default() += 1;
                }
            }
            for (k, n) in &tals {
                dump("tal", t, k, *n);
            }
            eprintln!("【武器 {}】有特效（前缀）{}；全能 {}；强攻 {}　【护符 {}】机制类 {}", TIER[t], pct(sp), pct(full), pct(atk), TIER[t], pct(tm));
        }
        if let Some(p) = std::env::var_os("DR_STATS_OUT") {
            fs::write(&p, &out).expect("写不了计数文件");
        }
    }

    #[test]
    fn buyback_steps() {
        let base = "container\tw\t240000000\t10000\t2\t10\t\ncontainer\tp\t30000000\t100\t2\t10\t\ncontainer\ta\t20000\t1\t2\t10\t\n\
                    text\tN\t128\ntext\tI\t160\ntext\tC\t640\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t0\t0\t8000\n\
                    deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\n";
        assert!(parse_items(base).unwrap().buyback.is_none());
        let it = parse_items(&format!("{base}wshop\t115000\t1\t0\nwshop\t115001\t1\t2\n")).unwrap();
        assert_eq!(it.wshop, vec![(115000, 1, 0), (115001, 1, 2)]);
        let it = parse_items(&format!("{base}buyback\t117000\t50\t93505\t93555\n")).unwrap();
        assert_eq!(it.buyback, Some(Buyback { row0: 117000, n: 50, rel0: 93505, stock0: 93555 }));
        assert!(parse_items(&format!("{base}buyback\t117000\t0\t93505\t93555\n")).unwrap().buyback.is_none());
        let a =|idx| Rec { cat: 2, idx, tier: 1, recipe: Recipe::A { tal: 9001, sp: 7200001, key: "x".into(), pos: 0 } };
        let mut st = St::default();
        for k in 0..5 {
            st.recs.insert((2, k), a(k));
        }
        let all: HashSet<(usize, u32)> = (0..5).map(|k| (2, k)).collect();
        // 第一次数：只记下来
        assert_eq!(bb_step(&mut st, None, &all, 3), (0, 0, 0));
        // 卖掉 1、3
        let now: HashSet<_> = all.iter().copied().filter(|k| k.1 != 1 && k.1 != 3).collect();
        assert_eq!(bb_step(&mut st, Some(&all), &now, 3), (0, 2, 0));
        assert_eq!(st.sold.iter().flatten().map(|r| r.idx).collect::<Vec<_>>(), vec![1, 3]);
        assert!(!st.recs.contains_key(&(2, 1)) && mine(&st, (2, 1)));
        // 容器留着：鉴定挑不到
        let held2 = now.clone();
        let defs_items = Items { cont: [Cont::default(), Cont::default(), Cont { first: 20000, step: 1, count: 5, size: 1, keep: vec![], second: None }], ..Default::default() };
        let dd = Defs { items: defs_items, lib: Lib::default() };
        assert_eq!(free(&dd, &st, &held2)[2], Vec::<u32>::new(), "0/2/4 在手上，1/3 在回购商店");
        // 买回 3：那格空着，别的不动
        let mut now2 = now.clone();
        now2.insert((2, 3));
        assert_eq!(bb_step(&mut st, Some(&now), &now2, 3), (1, 0, 0));
        assert_eq!(st.sold.iter().map(|s| s.as_ref().map(|r| r.idx)).collect::<Vec<_>>(), vec![Some(1), None]);
        assert!(st.recs.contains_key(&(2, 3)));
        // 再卖 0、2、4：挤紧、新的在前，超过 3 格挤掉最早的 1（容器空出来）
        let left: HashSet<_> = [(2, 3)].into_iter().collect();
        assert_eq!(bb_step(&mut st, Some(&now2), &left, 3), (0, 3, 1));
        assert_eq!(st.sold.iter().map(|s| s.as_ref().map(|r| r.idx)).collect::<Vec<_>>(), vec![Some(0), Some(2), Some(4)]);
        assert_eq!(st.evicted.iter().map(|r| r.idx).collect::<Vec<_>>(), vec![1], "挤掉的留下来另存");
        assert!(!mine(&st, (2, 1)));
        assert_eq!(free(&dd, &st, &left)[2], vec![1]);
    }

    #[test]
    fn locks() {
        let t = "# 注释\n2\t0\t240010000\t褪色者\n2\t1\t30000500\t褪色者\n3\t0\t240020000\t褪色者\n2\t2\t20001\t别人\n";
        let k = locked_keys(t, &(2, "褪色者".into()));
        assert_eq!(k, HashSet::from([(0, 240010000), (1, 30000500)]));
    }

    #[test]
    fn fit_text() {
        assert_eq!(String::from_utf16_lossy(&fit_cap("xy", 8)), "xy\0\0\0\0\0\0");
        assert_eq!(String::from_utf16_lossy(&fit_cap("0123456789", 8)), "01234567");
        assert_eq!(fit_cap("a😀", 2), vec![b'a' as u16, 0], "不拆开代理对");
    }

    #[test]
    fn lot_fill_rows() {
        let mut b = [0xAAu8; 152];
        lot_fill(&mut b, [0, 32, 64, 138, 128], Some((2, 240000000, 1)));
        assert_eq!(i32::from_le_bytes(b[0..4].try_into().unwrap()), 240000000);
        assert_eq!(i32::from_le_bytes(b[32..36].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(b[64..66].try_into().unwrap()), 1000);
        assert_eq!(b[138], 1);
        assert_eq!(b[96], 0xAA);
    }

    #[test]
    fn put_fields() {
        let mut lib = Lib::default();
        lib.fields.insert(('w', "x".into()), Field { off: 0, fmt: b'f', bit: -1 });
        lib.fields.insert(('w', "bit".into()), Field { off: 4, fmt: b'B', bit: 7 });
        lib.fields.insert(('w', "s".into()), Field { off: 5, fmt: b'i', bit: -1 });
        let mut row = [0u8; 9];
        put(&lib, 'w', &mut row, "x", 1.5);
        put(&lib, 'w', &mut row, "bit", 1.0);
        put(&lib, 'w', &mut row, "s", -1.0);
        assert_eq!(f32::from_le_bytes(row[0..4].try_into().unwrap()), 1.5);
        assert_eq!(row[4], 0x80);
        assert_eq!(i32::from_le_bytes(row[5..9].try_into().unwrap()), -1);
        put(&lib, 'w', &mut row, "bit", 0.0);
        assert_eq!(row[4], 0);
        // 强攻：五项基础攻击力（u16）按百分比乘，四舍五入；没有的字段不管
        let mut lib = Lib::default();
        for (k, nm) in ATTACK_BASE.iter().enumerate() {
            lib.fields.insert(('w', nm.to_string()), Field { off: 2 * k, fmt: b'H', bit: -1 });
        }
        let base: Vec<u8> = [100u16, 0, 87, 0, 1].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rec = Rec {
            cat: 0,
            idx: 0,
            tier: 2,
            recipe: Recipe::W { base: 1, var: 0, skill: -1, cor: [0.0; 5], req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 15, ovl: false, elem: 0, share: 0 },
        };
        let out = build_row(&lib, &rec, &base, false);
        let got: Vec<u16> = (0..5).map(|k| get_u16(&lib, 'w', &out, ATTACK_BASE[k]).unwrap()).collect();
        assert_eq!(got, vec![115, 0, 100, 0, 1]);
        // 轻 / 重武器：按本体的武器类型再乘，和强攻相乘
        lib.fields.insert(('w', "wepType".into()), Field { off: 10, fmt: b'H', bit: -1 });
        lib.wheavy = HashMap::from([(3, 1.2), (7, 1.5), (41, 1.5)]);
        let mut heavy = base.clone();
        heavy.extend(7u16.to_le_bytes());
        let out = build_row(&lib, &rec, &heavy, false);
        let got: Vec<u16> = (0..5).map(|k| get_u16(&lib, 'w', &out, ATTACK_BASE[k]).unwrap()).collect();
        assert_eq!(got, vec![173, 0, 150, 0, 2]); // 100 × (1.15 × 1.5 = 1.725)，四舍五入
        let mut light = base.clone();
        light.extend(3u16.to_le_bytes());
        let out = build_row(&lib, &rec, &light, false);
        let got: Vec<u16> = (0..5).map(|k| get_u16(&lib, 'w', &out, ATTACK_BASE[k]).unwrap()).collect();
        assert_eq!(got, vec![138, 0, 120, 0, 1]); // 100 × (1.15 × 1.2 = 1.38)，四舍五入
        // 致命一击按稀有度乘：短剑原本 130（throwAtkRate 30），这把档 2（史诗）× 1.6 = 208；没配就不动
        lib.fields.insert(('w', "throwAtkRate".into()), Field { off: 12, fmt: b'h', bit: -1 });
        let mut dagger = light.clone();
        dagger.extend(30i16.to_le_bytes());
        assert_eq!(get_u16(&lib, 'w', &build_row(&lib, &rec, &dagger, false), "throwAtkRate"), Some(30));
        lib.wcrit = [1.2, 1.4, 1.6, 1.8];
        assert_eq!(get_u16(&lib, 'w', &build_row(&lib, &rec, &dagger, false), "throwAtkRate"), Some(108));
    }

    /// 武器属性随机：五项基础攻击力重新分配（原版带元素的先折回纯物理）、九种结果各约 1/9、元素占比在区间里、传说光效跟着元素走
    #[test]
    fn weapon_element() {
        let w = WElem { on: true, lo: 30, hi: 60, coef: 0.3, pure: 1.08, skip: vec![57, 61] };
        let near = |a: [f64; 5], b: [f64; 5]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
        let phys = [100.0, 0.0, 0.0, 0.0, 0.0];
        assert!(near(elem_attack(&w, phys, 0, 0), phys), "0 = 照本体");
        assert!(near(elem_attack(&w, phys, 1, 0), phys));
        assert!(near(elem_attack(&w, phys, 3, 50), [53.75, 0.0, 53.75, 0.0, 0.0]), "五五开 × 1.075");
        assert!(near(elem_attack(&w, phys, 2, 30), [100.0 * 1.063 * 0.7, 100.0 * 1.063 * 0.3, 0.0, 0.0, 0.0]));
        assert!(near(elem_attack(&w, phys, 8, 100), [0.0, 0.0, 0.0, 108.0, 0.0]), "纯雷 × 1.08");
        assert!(near(elem_attack(&w, phys, 9, 100), [0.0, 0.0, 0.0, 0.0, 108.0]), "纯圣");
        // 原版物理 + 火 60 / 40：折回纯物理再分，照原样分回去还是原数
        let split = [60.0, 0.0, 40.0, 0.0, 0.0];
        assert!(near(elem_attack(&w, split, 1, 0), [100.0 / 1.072, 0.0, 0.0, 0.0, 0.0]));
        assert!(near(elem_attack(&w, split, 3, 40), split));
        assert!(near(elem_attack(&w, split, 2, 40), [60.0, 40.0, 0.0, 0.0, 0.0]), "换成魔力，数值不变");
        // 原版纯圣：÷ 1.08 折回纯物理
        assert!(near(elem_attack(&w, [0.0, 0.0, 0.0, 0.0, 108.0], 1, 0), phys));
        assert!(near(elem_attack(&w, [0.0, 0.0, 0.0, 0.0, 108.0], 6, 100), [0.0, 108.0, 0.0, 0.0, 0.0]));
        // 抽：九种结果各约 1/9，元素占比在 30~60，不抽的类型照本体
        let mut lib = Lib::default();
        lib.welem = w.clone();
        lib.glow = [-1, 11, 12, 13, 14];
        let mut rng = Rng(12345);
        let mut n = [0u32; 10];
        let n_draw = 90000;
        for _ in 0..n_draw {
            let (e, s, g) = draw_elem(&lib, 1, 3, 0, -1, &mut rng);
            n[e as usize] += 1;
            match e {
                1 => assert!(s == 0 && g == -1, "纯物理没有光效"),
                2..=5 => assert!((30..=60).contains(&s) && g == lib.glow[(e - 1) as usize]),
                6..=9 => assert!(s == 100 && g == lib.glow[(e - 5) as usize]),
                _ => panic!("elem {e}"),
            }
        }
        for e in 1..=9 {
            let f = n[e] as f64 / n_draw as f64;
            assert!((f - 1.0 / 9.0).abs() < 0.01, "elem {e} 占 {f:.3}");
        }
        assert_eq!(draw_elem(&lib, 57, 3, 2, 12, &mut rng), (0, 0, 12), "法杖不抽");
        let (e, _, g) = draw_elem(&lib, 1, 2, 0, -1, &mut rng);
        assert!(e > 0 && g == -1, "史诗没有光效");
        lib.welem.on = false;
        assert_eq!(draw_elem(&lib, 1, 3, 2, 12, &mut rng), (0, 0, 12), "没有 welem 行不抽");
        // build_row：先分配属性再乘强攻
        let mut lib = Lib::default();
        lib.welem = w;
        for (k, nm) in ATTACK_BASE.iter().enumerate() {
            lib.fields.insert(('w', nm.to_string()), Field { off: 2 * k, fmt: b'H', bit: -1 });
        }
        let base: Vec<u8> = [100u16, 0, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rec = |elem: u8, share: u8, atk: u8| Rec {
            cat: 0,
            idx: 0,
            tier: 2,
            recipe: Recipe::W { base: 1, var: 0, skill: -1, cor: [0.0; 5], req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk, ovl: false, elem, share },
        };
        let got = |r: &Rec| -> Vec<u16> {
            let out = build_row(&lib, r, &base, false);
            (0..5).map(|k| get_u16(&lib, 'w', &out, ATTACK_BASE[k]).unwrap()).collect()
        };
        assert_eq!(got(&rec(0, 0, 0)), vec![100, 0, 0, 0, 0]);
        assert_eq!(got(&rec(4, 50, 0)), vec![54, 0, 0, 54, 0]);
        assert_eq!(got(&rec(7, 100, 10)), vec![0, 0, 119, 0, 0]); // 108 × 1.1 = 118.8
        // 记录：有属性的多写三栏，读回来一样；旧记录没有这几栏 = 0
        let r = rec(5, 42, 0);
        assert_eq!(rec_parse(&rec_line(&r)).unwrap(), r);
        let line = rec_line(&rec(0, 0, 0));
        assert!(matches!(rec_parse(&line).unwrap().recipe, Recipe::W { elem: 0, share: 0, ovl: false, .. }));
    }

    /// 防具减伤倍率：在这个稀有度的区间里、同一件每次一样、不同件不一样，平均在中点附近；没有 pdef 照原版
    #[test]
    fn armor_negation_mult() {
        let mut lib = Lib::default();
        for (k, nm) in CUT_FIELDS.iter().enumerate() {
            lib.fields.insert(('p', nm.to_string()), Field { off: 4 * k, fmt: b'f', bit: -1 });
        }
        let base: Vec<u8> = [0.9f32, 0.88, 0.9, 0.92, 0.95, 0.96, 0.97, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rec = |idx: u32, tier: usize| Rec { cat: 1, idx, tier: tier as u8, recipe: Recipe::P { base: 470200, sps: vec![(7210425, "dr_bulwark".into())], mult: None, pos: Vec::new() } };
        let cut = |row: &[u8], k: usize| f32::from_le_bytes(row[4 * k..4 * k + 4].try_into().unwrap()) as f64;
        assert_eq!(build_row(&lib, &rec(0, 3), &base, false), base, "没有 pdef 照原版");
        lib.pdef = [Some((1.0, 1.2)), Some((1.1, 1.3)), Some((1.3, 1.5)), Some((1.4, 1.6))];
        for tier in 0..5 {
            let (lo, hi) = lib.pdef[tier.min(3)].unwrap();
            let mut sum = 0.0;
            let mut seen = HashSet::new();
            for idx in 0..400 {
                let row = build_row(&lib, &rec(idx, tier), &base, false);
                assert_eq!(row, build_row(&lib, &rec(idx, tier), &base, false), "同一件要一样");
                let m = (1.0 - cut(&row, 0)) / 0.1;
                assert!(m >= lo - 1e-4 && m <= hi + 1e-4, "档 {tier} 倍率 {m} 不在 {lo}~{hi}");
                for k in 1..7 {
                    let want = 1.0 - (1.0 - cut(&base, k)) * m;
                    assert!((cut(&row, k) - want).abs() < 1e-4, "第 {k} 种减伤没按同一个倍率乘");
                }
                assert_eq!(cut(&row, 7), 1.0, "原来 0% 的还是 0%");
                sum += m;
                seen.insert((m * 1e4) as i64);
            }
            let mean = sum / 400.0;
            assert!((mean - (lo + hi) / 2.0).abs() < 0.01, "档 {tier} 平均 {mean}");
            assert!(seen.len() > 300, "倍率要分散：{}", seen.len());
        }
    }

    /// 法器倍率：只有能施法的本体有；在稀有度区间里、同一件每次一样、平均在中点；参数行的补正值乘它；没有 cbase 都没有
    #[test]
    fn catalyst_base() {
        let mut lib = Lib::default();
        lib.wcast = HashMap::from([(33000000, (true, false)), (34000000, (false, true))]);
        let rec = |base: u32, idx: u32, tier: usize| Rec {
            cat: 0,
            idx,
            tier: tier as u8,
            recipe: Recipe::W { base, var: 0, skill: -1, cor: [0.0; 5], req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        assert_eq!(cast_base(&lib, &rec(33000000, 0, 3)), None, "没有 cbase 照原版");
        lib.cbase = [Some((70.0, 130.0)), Some((90.0, 150.0)), Some((110.0, 210.0)), Some((150.0, 250.0))];
        assert_eq!(cast_base(&lib, &rec(1000000, 0, 3)), None, "不是法器");
        let mut below = 0;
        for tier in 0..5 {
            let (lo, hi) = lib.cbase[tier.min(3)].unwrap();
            let mut sum = 0.0;
            for idx in 0..400 {
                let b = cast_base(&lib, &rec(34000000, idx, tier)).unwrap();
                assert_eq!(Some(b), cast_base(&lib, &rec(34000000, idx, tier)), "同一件要一样");
                assert!((lo..=hi).contains(&b), "档 {tier} 倍率 {b} 不在 {lo}~{hi}");
                below += (b < 100.0) as u32;
                sum += b;
            }
            assert!((sum / 400.0 - (lo + hi) / 2.0).abs() < 4.0, "档 {tier} 平均 {}", sum / 400.0);
        }
        assert!(below > 0, "普通、稀有的区间低于 100 的部分也要抽得到");
        let b = cast_base(&lib, &rec(33000000, 7, 3)).unwrap();
        // 参数行：五项补正值 × 倍率（配方不变）；不是法器的照配方
        for (k, nm) in ["correctStrength", "correctAgility", "correctMagic", "correctFaith", "correctLuck"].iter().enumerate() {
            lib.fields.insert(('w', nm.to_string()), Field { off: 4 * k, fmt: b'f', bit: -1 });
        }
        let with_cor = |base: u32, idx: u32| {
            let mut r = rec(base, idx, 3);
            if let Recipe::W { cor, .. } = &mut r.recipe {
                *cor = [0.0, 30.0, 120.0, 0.0, 50.0];
            }
            r
        };
        let got = |row: &[u8]| -> Vec<f32> { (0..5).map(|k| f32::from_le_bytes(row[4 * k..4 * k + 4].try_into().unwrap())).collect() };
        let row = build_row(&lib, &with_cor(33000000, 7), &[0u8; 20], false);
        let m = (b / 100.0) as f32;
        assert!(got(&row).iter().zip([0.0, 30.0, 120.0, 0.0, 50.0]).all(|(x, c)| (x - c * m).abs() < 1e-3), "{:?} × {m}", got(&row));
        assert_eq!(got(&build_row(&lib, &with_cor(1000000, 7), &[0u8; 20], false)), vec![0.0, 30.0, 120.0, 0.0, 50.0], "不是法器不乘");
    }

    /// 盾牌随机：六项各自落在这个稀有度的分位区间里（排好序的原版值线性插值）、同一件每次一样；不是盾 / 没有 wguard_q 照本体
    #[test]
    fn shield_guard_random() {
        let mut lib = Lib::default();
        lib.fields.insert(('w', "wepType".into()), Field { off: 0, fmt: b'H', bit: -1 });
        for (k, nm) in GUARD_F.iter().enumerate() {
            lib.fields.insert(('w', nm.to_string()), Field { off: 4 + 4 * k, fmt: if k == 5 { b'h' } else { b'f' }, bit: -1 });
        }
        let mut base = vec![0u8; 32];
        base[0..2].copy_from_slice(&67u16.to_le_bytes());
        for k in 0..5 {
            base[4 + 4 * k..8 + 4 * k].copy_from_slice(&50f32.to_le_bytes());
        }
        base[24..26].copy_from_slice(&55i16.to_le_bytes());
        let rec = |idx: u32, tier: usize| Rec {
            cat: 0,
            idx,
            tier: tier as u8,
            recipe: Recipe::W { base: 30000000, var: 0, skill: -1, cor: [0.0; 5], req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        let val = |row: &[u8], k: usize| {
            if k == 5 {
                i16::from_le_bytes(row[24..26].try_into().unwrap()) as f64
            } else {
                f32::from_le_bytes(row[4 + 4 * k..8 + 4 * k].try_into().unwrap()) as f64
            }
        };
        assert_eq!(build_row(&lib, &rec(0, 3), &base, false), base, "没有 wguard 照本体");
        // 减伤 30 ~ 90（11 个，等距）、防御强度 40 ~ 60：抽到的值 = 下限 + 跨度 × 分位
        lib.wguard.insert(67, std::array::from_fn(|k| if k == 5 { (40..=60).step_by(2).map(|x| x as f32).collect() } else { (0..11).map(|x| 30.0 + 6.0 * x as f32).collect() }));
        lib.wguard_q = [Some((0.0, 0.5)), Some((0.15, 0.7)), Some((0.3, 0.85)), Some((0.5, 1.0))];
        for tier in 0..4 {
            let (lo, hi) = lib.wguard_q[tier].unwrap();
            let mut seen = HashSet::new();
            for idx in 0..200 {
                let row = build_row(&lib, &rec(idx, tier), &base, false);
                assert_eq!(row, build_row(&lib, &rec(idx, tier), &base, false), "同一件要一样");
                for k in 0..5 {
                    let v = val(&row, k);
                    assert!(v >= 30.0 + 60.0 * lo - 0.06 && v <= 30.0 + 60.0 * hi + 0.06, "档 {tier} 第 {k} 项 {v} 不在分位 {lo}~{hi}");
                }
                let g = val(&row, 5);
                assert!(g >= 40.0 + 20.0 * lo - 0.5 && g <= 40.0 + 20.0 * hi + 0.5, "档 {tier} 防御强度 {g}");
                seen.insert(((val(&row, 1) * 10.0) as i64, (val(&row, 2) * 10.0) as i64));
            }
            assert!(seen.len() > 100, "各项要分散、互相独立：{}", seen.len());
        }
        let mut other = base.clone();
        other[0..2].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(build_row(&lib, &rec(1, 3), &other, false), other, "不是盾照本体");
    }

    /// 生成武器都能附魔：从原本不可附魔的本体复制后，build_row 仍要把 isEnhance 置 1
    #[test]
    fn generated_weapons_are_buffable() {
        let mut lib = Lib::default();
        lib.fields.insert(('w', "isEnhance".into()), Field { off: 0, fmt: b'B', bit: 7 });
        let rec = Rec {
            cat: 0,
            idx: 0,
            tier: 1,
            recipe: Recipe::W { base: 1, var: 0, skill: -1, cor: [0.0; 5], req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        let row = build_row(&lib, &rec, &[0u8; 1], false);
        assert_eq!(row[0] & 0x80, 0x80);
    }

    /// 稀有度边框：图标按稀有度换成带框的，防具男女两个都换；frames.txt 里没有的照原版
    #[test]
    fn rarity_frame_icons() {
        let mut lib = Lib::default();
        lib.fields.insert(('p', "iconIdM".into()), Field { off: 0, fmt: b'H', bit: -1 });
        lib.fields.insert(('p', "iconIdF".into()), Field { off: 2, fmt: b'H', bit: -1 });
        lib.ficon = parse_frames("# 注释\n13953\t3\t51019\n13953 0 51016\n9999 9 1\nxx\n");
        assert_eq!(lib.ficon.len(), 2);
        let base: Vec<u8> = [13953u16, 13953].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rec = |tier| Rec { cat: 1, idx: 0, tier, recipe: Recipe::P { base: 1, sps: vec![], mult: None, pos: Vec::new() } };
        let got = |row: Vec<u8>| (u16::from_le_bytes([row[0], row[1]]), u16::from_le_bytes([row[2], row[3]]));
        assert_eq!(got(build_row(&lib, &rec(3), &base, false)), (51019, 51019));
        assert_eq!(got(build_row(&lib, &rec(4), &base, false)), (51019, 51019), "幽影之地的传说照传说");
        assert_eq!(got(build_row(&lib, &rec(1), &base, false)), (13953, 13953));
        // 护符模板行（frame_templates）：换一次带框；再换（或容器抄了带框的模板行再 build_row）照旧
        lib.fields.insert(('a', "iconId".into()), Field { off: 0, fmt: b'H', bit: -1 });
        lib.ficon.insert((4200, 1), 50010);
        let mut tal = 4200u16.to_le_bytes().to_vec();
        assert_eq!(frame_row(&lib, 2, &mut tal, 1), 1);
        assert_eq!(u16::from_le_bytes([tal[0], tal[1]]), 50010);
        assert_eq!(frame_row(&lib, 2, &mut tal, 1), 0);
        let trec = Rec { cat: 2, idx: 0, tier: 1, recipe: Recipe::A { tal: 1, sp: -1, key: String::new(), pos: 0 } };
        let row = build_row(&lib, &trec, &tal, false);
        assert_eq!(u16::from_le_bytes([row[0], row[1]]), 50010);
    }

    /// 全能补正倍率（womni，用户 2026-11-24）：五项都有补正的参数行补正 × womni，别的不乘；没有 womni 行是 1；法器倍率照样乘在一起
    #[test]
    fn omni_correct_mult() {
        let mut lib = Lib::default();
        for (k, s) in ["Strength", "Agility", "Magic", "Faith", "Luck"].iter().enumerate() {
            lib.fields.insert(('w', format!("correct{s}")), Field { off: 4 * k, fmt: b'f', bit: -1 });
        }
        let rec = |base: u32, cor: [f32; 5]| Rec {
            cat: 0,
            idx: 3,
            tier: 3,
            recipe: Recipe::W { base, var: 0, skill: -1, cor, req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        let cors = |row: Vec<u8>| -> Vec<f32> { (0..5).map(|k| f32::from_le_bytes(row[4 * k..4 * k + 4].try_into().unwrap())).collect() };
        let base = vec![0u8; 20];
        assert_eq!(cors(build_row(&lib, &rec(1, [100.0; 5]), &base, false)), vec![100.0; 5], "没有 womni 不乘");
        lib.womni = Some(0.75);
        assert_eq!(cors(build_row(&lib, &rec(1, [100.0; 5]), &base, false)), vec![75.0; 5]);
        assert_eq!(cors(build_row(&lib, &rec(1, [100.0, 80.0, 60.0, 40.0, 0.0]), &base, false)), vec![100.0, 80.0, 60.0, 40.0, 0.0], "四项不乘");
        assert_eq!(omni_mult(&lib, &[1.0; 5]), 0.75);
        // 法器（wcast + cbase）：法器倍率 × 全能补正倍率
        lib.wcast.insert(7, (true, false));
        lib.cbase[3] = Some((110.0, 110.0));
        let got = cors(build_row(&lib, &rec(7, [100.0; 5]), &base, false));
        assert!(got.iter().all(|&c| (c - 82.5).abs() < 1e-3), "{got:?}");
    }

    /// 强化：按本体的强化类型、稀有度、全能换；已经强到上限以上的（old）、没有对应的照原版
    #[test]
    fn reinforce_type_by_rarity() {
        let mut lib = Lib::default();
        lib.fields.insert(('w', "reinforceTypeId".into()), Field { off: 0, fmt: b'h', bit: -1 });
        lib.fields.insert(('w', "materialSetId".into()), Field { off: 4, fmt: b'i', bit: -1 });
        lib.reinf.insert((2200, 2, false), 10500);
        lib.reinf.insert((2200, 2, true), 10600);
        lib.rmat = [9900000, 9900010, 9900020, 9900030];
        lib.rmax = 5;
        let base: Vec<u8> = [2200i16.to_le_bytes().to_vec(), vec![0; 2], 2200i32.to_le_bytes().to_vec()].concat();
        let rec = |cor: [f32; 5]| Rec {
            cat: 0,
            idx: 0,
            tier: 2,
            recipe: Recipe::W { base: 1, var: 0, skill: -1, cor, req: [0; 5], curve: [0; CURVE_N], st: [-1; 3], res: [-1; 2], glow: -1, spec: -1, atk: 0, ovl: false, elem: 0, share: 0 },
        };
        let got = |row: Vec<u8>| (i16::from_le_bytes([row[0], row[1]]), i32::from_le_bytes(row[4..8].try_into().unwrap()));
        assert_eq!(got(build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &base, false)), (10500, 9900020));
        assert_eq!(got(build_row(&lib, &rec([50.0; 5]), &base, false)), (10600, 9900020));
        assert_eq!(got(build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &base, true)), (2200, 2200));
        let other: Vec<u8> = [0i16.to_le_bytes().to_vec(), vec![0; 6]].concat();
        assert_eq!(got(build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &other, false)), (0, 0));

        // 强化上限：换了强化类型的，originEquipWep6~25 写 -1；没换的（old、本体不能强化的）26 个都留着
        let keep: Vec<(usize, usize)> = (0..26).map(|k| (8 + 4 * k, 4)).collect();
        let origin = |row: &[u8]| (0..26).map(|k| i32::from_le_bytes(row[8 + 4 * k..12 + 4 * k].try_into().unwrap())).collect::<Vec<_>>();
        let pad = |r: Vec<u8>| [r, 777i32.to_le_bytes().repeat(26)].concat();
        let base = pad(base);
        let mut row = pad(build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &base[..8], false));
        cap_origin(&lib, &keep, &base, &mut row);
        assert_eq!(origin(&row), [vec![777; 6], vec![-1; 20]].concat());
        let mut row = pad(build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &base[..8], true));
        cap_origin(&lib, &keep, &base, &mut row);
        assert_eq!(origin(&row), vec![777; 26]);
        // 照原版不能强化的本体（陨石杖）复制的行（开局装备 / 追忆武器）：1~25 原来是 -1，换了强化类型后 1~5 指自己
        let mut row = [build_row(&lib, &rec([50.0, 30.0, 0.0, 0.0, 0.0]), &base[..8], false), 777i32.to_le_bytes().to_vec(), (-1i32).to_le_bytes().repeat(25)].concat();
        cap_origin(&lib, &keep, &base, &mut row);
        assert_eq!(origin(&row), [vec![777; 6], vec![-1; 20]].concat());
    }

    /// 用重随写出来的真文件：解析、每类每档生成很多件，检查都合规、文字放得进占位
    #[test]
    fn start_items_from_real_defs() {
        let Some((items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\items.txt / affixes.txt（先跑一次重随），跳过");
            return;
        };
        if items.start.is_empty() {
            eprintln!("items.txt 里没有 start 行，跳过");
            return;
        }
        for (k, s) in items.start.iter().enumerate() {
            let make = |seed| {
                let mut rng = Rng(seed);
                match s.cat {
                    0 => gen_weapon_on(&lib, s.tier as usize, Some(s.wfix()), &mut rng),
                    _ => gen_armor_on(&lib, s.tier as usize, Some(s.base), s.full, &mut rng),
                }
            };
            let r = make(s.seed).unwrap_or_else(|| panic!("开局装备 {} 生成失败", s.id));
            assert_eq!(make(s.seed), Some(r.clone()), "同一个种子结果要一样");
            assert_eq!(base_of(&Rec { cat: s.cat, idx: 0, tier: s.tier, recipe: r.clone() }), s.base);
            if let Recipe::P { sps, .. } = &r {
                if s.full {
                    assert_eq!(sps.len(), 3, "{} 强制三词条的防具只生成了 {} 条", s.id, sps.len());
                }
            }
            if let Recipe::W { req, st, cor, .. } = &r {
                assert!((0..5).all(|j| req[j] <= s.maxreq[j]), "{} 能力需求超过职业属性：{req:?} > {:?}", s.id, s.maxreq);
                let n = cor.iter().filter(|&&c| c > 0.0).count() as u32;
                let rr = (s.tier as usize).min(3);
                if let Some(d) = lib.wscal_v.get(&(rr, n)).or_else(|| lib.wscal_v.get(&(rr, 2))) {
                    let lo = d.top(s.top).items.iter().map(|x| x.0).fold(f64::MAX, f64::min);
                    assert!(cor.iter().all(|&c| c == 0.0 || c as f64 >= lo - 1e-3), "{} 补正不在前 {}%：{cor:?} < {lo}", s.id, s.top * 100.0);
                }
                let ns = st.iter().filter(|&&x| x > 0).count();
                if s.min_status < 0 {
                    assert_eq!(ns, 0, "{} 不该带异常：{st:?}", s.id);
                } else {
                    assert!(ns >= (s.min_status as usize).min(3), "{} 异常不够 {} 种：{st:?}", s.id, s.min_status);
                }
                assert!(n < 5 || s.keep || s.full, "{} 开局武器抽到了全能：{cor:?}", s.id);
                if let (true, Recipe::W { skill, res, var, .. }) = (s.keep, &r) {
                    let b = lib.wbase.iter().find(|b| b.id == s.base).unwrap();
                    assert_eq!((*skill, *res, *var), (-1, [KEEP_RES; 2], b.var), "{} 追忆武器的战技 / 常驻效果要照本体", s.id);
                    assert!(need_level(req) >= lib.wreq_minlvl[3], "{} 追忆武器不够 {} 级：{req:?}", s.id, lib.wreq_minlvl[3]);
                }
                assert!(ordered(cor, req) || !lib.wreq_sorted, "{} 需求和补正排序不同：{cor:?} / {req:?}", s.id);
                let top = |c: &[f32; 5]| c.iter().cloned().fold(f32::MIN, f32::max);
                if s.main >= 0 {
                    assert_eq!(cor[s.main as usize], top(cor), "{} 主补正不在职业最高的属性 {} 上：{cor:?}", s.id, s.main);
                }
                if !s.keep {
                    for m in 0..5 {
                        let f = WFix { main: m, ..s.wfix() };
                        for seed in 0..20u64 {
                            if let Some(Recipe::W { cor, req, .. }) = gen_weapon_on(&lib, s.tier as usize, Some(f), &mut Rng(seed)) {
                                assert_eq!(cor[m as usize], top(&cor), "{} 主补正指定 {m}：{cor:?}", s.id);
                                assert!(ordered(&cor, &req) || !lib.wreq_sorted, "{} 指定 {m} 后需求和补正排序不同：{cor:?} / {req:?}", s.id);
                            }
                        }
                    }
                }
                if k < 4 {
                    eprintln!("开局武器 {}（本体 {}）：补正 {cor:?}，异常 {st:?}", s.id, s.base);
                }
                // 面板「附加效果」的文字跟着每个异常槽位
                let row = build_row(&lib, &Rec { cat: 0, idx: 0, tier: s.tier, recipe: r.clone() }, &vec![0u8; items.cont[0].size], false);
                for j in 0..3 {
                    let f = lib.fields[&('w', format!("spEffectMsgId{j}"))];
                    let msg = i32::from_le_bytes(row[f.off..f.off + 4].try_into().unwrap());
                    assert_eq!(msg > 0, st[j] > 0, "{} 第 {j} 个异常的面板文字 {msg}（异常 {}）", s.id, st[j]);
                }
            }
            let rec = Rec { cat: s.cat, idx: start_off(&items, s.cat, s.id).expect("start 行的编号不在 startids 编号段里"), tier: s.tier, recipe: r };
            assert_eq!(rec_id(&items, &rec), s.id);
            for lang in 0..2 {
                let t = compose_cap(&lib, &rec, lang, items.cap[1]);
                assert!((0..3).all(|f| t[f].encode_utf16().count() <= items.cap[f]), "{} 文字超长", s.id);
            }
        }
    }

    /// 补正高的那项需求不低于补正低的
    fn ordered(cor: &[f32; 5], req: &[u8; 5]) -> bool {
        (0..5).all(|a| (0..5).all(|b| !(cor[a] > 0.0 && cor[b] > 0.0 && cor[a] > cor[b] && req[a] < req[b])))
    }

    #[test]
    fn armor_affixes_respect_slots() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\affixes.txt（先跑一次重随），跳过");
            return;
        };
        let mut rng = Rng(11);
        let mut seen: HashMap<String, [u32; 4]> = HashMap::new();
        for _ in 0..20000 {
            let Some(Recipe::P { base, sps, .. }) = gen_armor(&lib, 3, &mut rng) else { panic!("防具生成失败") };
            let slot = lib.pbase.iter().find(|b| b.id == base).unwrap().slot;
            for (_sp, key) in &sps {
                let f = &lib.fams[lib.fam_by_key[key]];
                assert!(f.slots & (1 << slot) != 0, "词条 {key} 出在了部位 {slot}（只能 {:b}）", f.slots);
                seen.entry(key.clone()).or_default()[slot as usize] += 1;
            }
        }
        for key in ["dr_leap", "dr_momentum", "dr_elegance", "dr_cocktail", "dr_gale", "dr_wolf"] {
            if lib.fam_by_key.contains_key(key) {
                eprintln!("{key}：头 / 身 / 手 / 腿 {:?}", seen.get(key).copied().unwrap_or_default());
                assert!(seen.get(key).is_some_and(|v| v.iter().sum::<u32>() > 0), "新词条 {key} 一次都没出");
            }
        }
    }

    #[test]
    fn wish_defs_and_map_bands() {
        let Some((items, _lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\items.txt（先跑一次重随），跳过");
            return;
        };
        let Some(w) = items.wish.as_ref() else {
            eprintln!("items.txt 里没有 wish 行，跳过");
            return;
        };
        assert!(w.flag1 > 0 && w.flag10 > 0 && w.free_n <= 30 && w.share.iter().sum::<u32>() > 0);
        assert!(w.bands.iter().all(|b| b.cost > 0 && b.w.iter().sum::<u32>() > 0));
        // 宁姆格福（m60_42_36_00）是前期，DLC 的大地图在 DLC 档；大地图没列的小图块按上两级图块找
        let lim = w.band_at(60, 42, 36, 0).expect("宁姆格福没有档");
        assert_eq!(lim, 0);
        let dlc = w.maps.iter().find(|(k, _)| k.0 == 61).map(|(k, _)| *k).expect("没有 DLC 的地图");
        assert!(w.bands[w.band_at(dlc.0, dlc.1, dlc.2, dlc.3).unwrap()].dlc);
        for (k, &b) in w.maps.iter().filter(|(k, _)| k.0 == 60 && k.3 == 2) {
            let (x, y) = (k.1 * 4 + 1, k.2 * 4 + 2);
            if !w.maps.contains_key(&(60, x, y, 0)) && !w.maps.contains_key(&(60, x / 2, y / 2, 1)) {
                assert_eq!(w.band_at(60, x, y, 0), Some(b), "小图块 m60_{x}_{y}_00 要按上两级图块 {k:?} 找");
            }
        }
        assert_eq!(w.band_at(10, 0, 0, 0), w.maps.get(&(10, 0, 0, 0)).copied());
        eprintln!("许愿：{} 档，价格 {:?}，{} 张地图", w.bands.len(), w.bands.iter().map(|b| b.cost).collect::<Vec<_>>(), w.maps.len());
    }

    #[test]
    fn weapon_rules_from_real_library() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\affixes.txt（先跑一次重随），跳过");
            return;
        };
        let mut rng = Rng(7);
        for tier in 0..5 {
            let r = tier.min(3);
            let mut lv = Vec::new();
            let mut full = 0;
            for _ in 0..4000 {
                let recipe = gen_weapon(&lib, tier, &mut rng);
                let name = compose_cap(&lib, &Rec { cat: 0, idx: 0, tier: tier as u8, recipe: recipe.clone().unwrap() }, 0, 160)[0].clone();
                let Some(Recipe::W { cor, req, st, .. }) = recipe else { panic!("档 {tier} 生成失败") };
                let n_cor = cor.iter().filter(|&&c| c > 0.0).count();
                assert_eq!(name.contains("全能"), n_cor == 5, "名字只在全能时带「全能」：{name}");
                assert!(!lib.stat.iter().any(|s| name.contains(&format!("{}/", s[0]))), "名字里不该有补正：{name}");
                if lib.wreq_sorted {
                    assert!(ordered(&cor, &req), "档 {tier} 需求和补正排序不同：{cor:?} / {req:?}");
                }
                assert!((0..5).all(|k| cor[k] > 0.0 || req[k] == 0), "没补正的项有需求：{cor:?} / {req:?}");
                assert!(need_level(&req) >= lib.wreq_minlvl[r], "档 {tier} 等级 {} 不够 {}：{req:?}", need_level(&req), lib.wreq_minlvl[r]);
                if cor.iter().filter(|&&c| c > 0.0).count() == 5 {
                    full += 1;
                    assert!(!lib.full_no_status || st.iter().all(|&s| s <= 0), "全能带了异常：{st:?}");
                }
                lv.push(need_level(&req));
            }
            lv.sort_unstable();
            // 特殊效果词条位：只出特效池里的、这个稀有度的出现率大致对得上
            let mut rng2 = Rng(99);
            let specs: Vec<i32> = (0..4000).filter_map(|_| match gen_weapon(&lib, tier, &mut rng2) {
                Some(Recipe::W { spec, .. }) if spec > 0 => Some(spec),
                _ => None,
            }).collect();
            assert!(specs.iter().all(|s| lib.wspec.iter().any(|x| x.sp == *s)), "特效不在池里：{specs:?}");
            let want = match lib.wspec_p[tier.min(4)] {
                Some(p) => p as f64 / 100.0,
                None => lib.wspec_n[r].items.iter().filter(|x| x.0 >= 1.0).map(|x| x.1 as f64).sum::<f64>() / lib.wspec_n[r].total.max(1) as f64,
            };
            let got = specs.len() as f64 / 4000.0;
            assert!((got - want).abs() < 0.03, "档 {tier} 特效出现率 {got:.3}，应该 {want:.3}");
            let q = |p: f64| lv[((lv.len() - 1) as f64 * p) as usize];
            eprintln!("档 {tier}：要的等级 最低 {} / 5% {} / 中位 {} / 95% {} / 最高 {}（全能 {full} 把）", q(0.0), q(0.05), q(0.5), q(0.95), q(1.0));
        }
        // 鉴定偏向：纯力气加点，力气有补正的比例要明显比不偏向的高
        if lib.bias_n > 1 {
            let s = [40, 10, 10, 10, 10];
            let str_share = |biased: bool, rng: &mut Rng| {
                let n = 3000;
                let hit = (0..n)
                    .filter(|_| {
                        let w = if biased { gen_weapon_for(&lib, 2, Some(s), rng) } else { gen_weapon(&lib, 2, rng) };
                        matches!(w, Some(Recipe::W { cor, .. }) if cor[0] > 0.0)
                    })
                    .count();
                hit as f64 / n as f64
            };
            let (plain, biased) = (str_share(false, &mut rng), str_share(true, &mut rng));
            eprintln!("鉴定偏向（力气 40，其余 10，史诗）：带力气补正 {:.0}% → {:.0}%", plain * 100.0, biased * 100.0);
            assert!(biased > plain + 0.1);
        }
    }

    /// 锁定 → 不能卖：新锁的、被重建回原价的改成 -1（记下原价），本来就是 -1 的不动，解锁的改回原价
    #[test]
    fn unsell_steps() {
        let prices: HashMap<(usize, u32), i32> = [((0, 240000000), 500), ((1, 30000100), 300), ((0, 17030000), -1), ((2, 1000), 800)].into();
        let mut applied = HashMap::new();
        let want: HashSet<_> = [(0, 240000000), (1, 30000100), (0, 17030000), (0, 999)].into();
        let (r, h) = unsell_step(&applied, &want, |k| prices.get(&k).copied());
        assert!(r.is_empty());
        assert_eq!(h, vec![((0, 240000000), 500), ((1, 30000100), 300)], "本来 -1 的、没有参数行的不动");
        applied.extend(h);
        // 改成 -1 以后：没变化；武器那行被重建回 520：再改、记新价；防具解锁：改回 300
        let after: HashMap<_, _> = [((0, 240000000), 520), ((1, 30000100), -1)].into();
        let want2: HashSet<_> = [(0, 240000000)].into();
        let (r, h) = unsell_step(&applied, &want2, |k| after.get(&k).copied());
        assert_eq!((r, h), (vec![((1, 30000100), 300)], vec![((0, 240000000), 520)]));
    }

    /// 容器两段编号：第一段到 split 为止，后面接第二段；编号和序号互换，段外的不认
    #[test]
    fn container_two_segments() {
        let it = parse_items(
            "container\tw\t240000000\t10000\t3000\t664\t96:4\t200000000\t2843\ncontainer\tp\t30000000\t100\t6000\t416\t\n\
             container\ta\t20000\t1\t600\t96\t\ntext\tN\t128\ntext\tI\t160\ntext\tC\t640\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t0\t0\t8000\n\
             deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\n",
        )
        .unwrap();
        let (w, p) = (&it.cont[0], &it.cont[1]);
        assert_eq!(w.second, Some((200000000, 2843)));
        assert_eq!((w.id(0), w.id(2842), w.id(2843), w.id(2999)), (240000000, 268420000, 200000000, 201560000));
        for k in [0, 1499, 1500, 2842, 2843, 2999] {
            assert_eq!(w.idx_of(w.id(k)), Some(k));
        }
        assert_eq!(w.idx_of(201570000), None, "第二段超出个数");
        assert_eq!(w.idx_of(240005000), None, "不在间隔上");
        assert_eq!((p.second, p.id(5999), p.idx_of(30599900), p.idx_of(30600000)), (None, 30599900, Some(5999), None));
    }

    /// 装备继承（装备工坊）：items.txt 的 inherit 行；各种放法（部位限制、机制类进前缀 / 数值类进后缀、空位补上、满了分别替换、
    /// 同一条 / 同组不行、目标满了没位置）、能不能提取成护符；词条带别的稀有度的效果时说明按那个稀有度写、注明
    #[test]
    fn inherit_options_from_real_library() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\items.txt / affixes.txt（先跑一次重随），跳过");
            return;
        };
        let items = parse_items(
            "container\tw\t240000000\t10000\t3000\t664\t96:4\ncontainer\tp\t30000000\t100\t6000\t416\t\n\
             container\ta\t20000\t1\t600\t96\t\ntext\tN\t192\ntext\tI\t160\ntext\tC\t896\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t0\t0\t8000\n\
             deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\ninherit\t93674\t93675\t93676\t9241\t93091\t93092\n\
             dirident\t93634\t93635\t6\t93641\t3\t93644\t10,100,500,1000,2000\t1,3,5\n",
        )
        .unwrap();
        assert_eq!(items.inherit, Some(Inherit { open: 93674, ack: 93675, noui: 93676, goods: 9241, pages: [93091, 93092] }));
        assert_eq!(items.dirid, Some(DirId { flag: 93634, type0: (93635, 6), rar0: (93641, 3), none: 93644, cost: [10, 100, 500, 1000, 2000], types: vec![1, 3, 5],
                                             kind0: (0, 0), cost_t: [10, 100, 500, 1000, 2000], archs: vec![] }), "旧的 dirident 行：只有武器，护符同价");
        assert_eq!(parse_custom_icons("# x\n3230\t1\t50001\ncustom\tfusion_heart\t56000\ncustom\tbad\tx\n").get("fusion_heart"), Some(&56000));
        assert_eq!(parse_frames("custom\tfusion_heart\t56000\n3230\t1\t50001\n").len(), 1);
        let d = Defs { items, lib };
        let lib = &d.lib;
        // 挑词条：全部位、不在组里的机制类 2 条、数值类 3 条；只出在手上的机制类；同组的两条
        let usable = |f: &&Fam| f.r[3].is_some() && f.r[0].is_some() && f.armor_w.iter().any(|&w| w > 0);
        let pick = |mech: bool, n: usize| -> Vec<&Fam> { lib.fams.iter().filter(usable).filter(|f| f.mech == mech && f.slots == 15 && f.group.is_empty()).take(n).collect() };
        let m = pick(true, 2);
        let v = pick(false, 3);
        assert!(m.len() == 2 && v.len() == 3);
        let hand = lib.fams.iter().filter(usable).find(|f| f.mech && f.slots == 4).expect("没有只出在手上的机制类词条");
        let grp = lib.fams.iter().filter(usable).filter(|f| !f.group.is_empty() && f.slots == 15 && !f.mech).collect::<Vec<_>>();
        let (g1, g2) = grp.iter().flat_map(|a| grp.iter().map(move |b| (*a, *b))).find(|(a, b)| a.key != b.key && a.group == b.group).expect("没有同组的两条数值类");
        let sp = |f: &Fam, r: usize| f.r[r].as_ref().unwrap().sp;
        let p = |f: &Fam, r: usize| (sp(f, r), f.key.clone());
        let base_of_slot = |s: u8| lib.pbase.iter().find(|b| b.slot == s).unwrap().id;
        let rec = |idx: u32, slot: u8, tier: u8, sps: Vec<(i32, String)>| Rec { cat: 1, idx, tier, recipe: Recipe::P { base: base_of_slot(slot), sps, mult: None, pos: Vec::new() } };
        let head_full = rec(10, 0, 3, vec![p(m[0], 3), p(v[0], 3), p(v[1], 3)]);
        let body_empty = rec(11, 1, 3, vec![]);
        let hands_one = rec(12, 2, 3, vec![p(v[0], 3)]);
        let legs_three_mech_free = rec(13, 3, 3, vec![p(v[0], 3), p(v[1], 3), p(v[2], 3)]);
        // 来源（普通）：机制 m1、数值 v2、手专属机制 hand、数值 v0、同组 g2
        let src = rec(20, 1, 0, vec![p(m[1], 0), p(v[2], 0), p(hand, 0)]);
        let ok = |reps: &[Option<usize>]| Ok(reps.iter().map(|&r| (r, None)).collect::<Vec<_>>());
        // 机制类：头替换前缀（第 0 条）、身补上、手补上；腿三条都是数值类、没有前缀可换 → 整件不行
        assert_eq!(ih_options(lib, &src, 0, &head_full), ok(&[Some(0)]));
        assert_eq!(ih_options(lib, &src, 0, &body_empty), ok(&[None]));
        assert_eq!(ih_options(lib, &src, 0, &hands_one), ok(&[None]));
        assert!(ih_options(lib, &src, 0, &legs_three_mech_free).is_err());
        // 数值类：头后缀满了 → 分别替换第 1 / 第 2 条；身补上；手只有一条后缀 → 补上；腿三条后缀 → 三条都能换
        assert_eq!(ih_options(lib, &src, 1, &head_full), ok(&[Some(1), Some(2)]));
        assert_eq!(ih_options(lib, &src, 1, &body_empty), ok(&[None]));
        assert_eq!(ih_options(lib, &src, 1, &hands_one), ok(&[None]));
        // v2 已经在腿上（第 3 条）：替换前两条时留下的里有它 → 不行；替换它自己那条可以
        let legs = ih_options(lib, &src, 1, &legs_three_mech_free).unwrap();
        assert!(legs.len() == 3 && legs[0].1.is_some() && legs[1].1.is_some() && legs[2] == (Some(2), None), "{legs:?}");
        // 手专属：只能去手
        assert!(ih_options(lib, &src, 2, &head_full).is_err() && ih_options(lib, &src, 2, &body_empty).is_err());
        assert_eq!(ih_options(lib, &src, 2, &hands_one), ok(&[None]));
        // 同一条：来源 v0、手上有 v0 → 补上也不行（留下的里有它）
        let src2 = rec(21, 1, 0, vec![p(v[0], 0)]);
        assert!(matches!(ih_options(lib, &src2, 0, &hands_one).unwrap().as_slice(), [(None, Some(_))]));
        // 同组：头 [m0, g1, v1]、来源 g2 → 替换 g1 可以，替换 v1 不行（留下的 g1 和它同组）
        let head_g = rec(14, 0, 3, vec![p(m[0], 3), p(g1, 3), p(v[1], 3)]);
        let src3 = rec(22, 1, 0, vec![p(g2, 0)]);
        let o = ih_options(lib, &src3, 0, &head_g).unwrap();
        assert!(o.len() == 2 && o[0] == (Some(1), None) && o[1].0 == Some(2) && o[1].1.is_some(), "{o:?}");
        // 提取成护符：这条词条在它的稀有度有护符才行
        for f in [m[1], v[2], hand] {
            assert_eq!(ih_talisman(lib, &f.key, 0).is_some(), f.r[0].as_ref().unwrap().tal >= 0, "{}", f.key);
        }
        // 说明：每条词条按自己的稀有度写、上自己稀有度的颜色（传说防具上转过来的普通词条是普通的颜色），不再注明
        let mixed = rec(30, 0, 3, vec![p(m[0], 0), p(v[0], 3)]);
        let cap = compose_cap(lib, &mixed, 0, 160)[2].clone();
        let first = |s: String| s.split('\n').next().unwrap_or_default().to_string();
        assert!(cap.contains(&format!("<font color=\"#DADADA\">{}</font>", first(sentence(lib, &m[0].key, 0, 0)))), "{cap}");
        assert!(cap.contains(&format!("<font color=\"#D4A900\">{}</font>", first(sentence(lib, &v[0].key, 3, 0)))), "{cap}");
        assert!(!cap.contains("数值）"), "{cap}");
        assert_eq!(strip_font(&cap), format!("{}\n{}", sentence(lib, &m[0].key, 0, 0), sentence(lib, &v[0].key, 3, 0)));
        assert_eq!(sp_rar(lib, &m[0].key, sp(m[0], 0)), Some(0));
    }

    /// items.txt 的重铸石、回收行（rr/reforge.py）；没有这两行就都是 None
    #[test]
    fn reforge_recycle_defs() {
        let head = "container\tw\t240000000\t10000\t3000\t664\t96:4\ncontainer\tp\t30000000\t100\t6000\t416\t\n\
             container\ta\t20000\t1\t600\t96\t\ntext\tN\t128\ntext\tI\t160\ntext\tC\t640\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t0\t0\t8000\n\
             deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\n";
        let it = parse_items(&format!("{head}reforge\t9240\t369180\t369181\nrecycle\t9240\t93087,93088,93089,93090\t1,2,3,4\t93091\n")).unwrap();
        assert_eq!(it.reforge, Some(Reforge { goods: 9240, mark: 369180, fx: 369181 }));
        assert_eq!(it.recycle, Some(Recycle { goods: 9240, flags: [93087, 93088, 93089, 93090], stones: [1, 2, 3, 4], discard: 93091 }));
        // 旧 items.txt 没有第 4 个数（回收全部弃置是 2026-11-26 加的）：照旧解析，不等于没有这个功能
        let it = parse_items(&format!("{head}recycle\t9240\t93087,93088,93089,93090\t1,2,3,4\n")).unwrap();
        assert_eq!(it.recycle, Some(Recycle { goods: 9240, flags: [93087, 93088, 93089, 93090], stones: [1, 2, 3, 4], discard: 0 }));
        let it = parse_items(head).unwrap();
        assert_eq!((it.reforge, it.recycle), (None, None));
    }

    /// 非全能武器重铸出全能的概率固定是 1 / REFORGE_OMNI_ODDS：不看稀有度和加点，带不带异常都一样
    #[test]
    fn reforge_omni_odds_flat() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\affixes.txt（先跑一次重随），跳过");
            return;
        };
        let mut rng = Rng(41);
        const N: u32 = 40_000;
        let want = N as f64 / REFORGE_OMNI_ODDS as f64;
        for (tier, status) in (0..4).flat_map(|t| [(t, false), (t, true)]) {
            let w = loop {
                let w = gen_weapon(&lib, tier, &mut rng).unwrap();
                let Recipe::W { cor, st, .. } = &w else { continue };
                if !cor.iter().all(|&c| c > 0.0) && st.iter().any(|&s| s > 0) == status {
                    break w;
                }
            };
            for s in [None, Some([10; 5]), Some([80, 10, 10, 10, 10]), Some([10, 10, 60, 60, 10]), Some([20; 5]), Some([99; 5])] {
                let hit = (0..N)
                    .filter(|_| matches!(reforge_recipe(&lib, tier, &w, s, &mut rng), Some(Recipe::W { cor, .. }) if cor.iter().all(|&c| c > 0.0)))
                    .count() as f64;
                assert!((hit - want).abs() < want * 0.2, "档 {tier} 加点 {s:?}：{hit} 次，应该约 {want}");
            }
        }
    }

    /// 防具 / 护符鉴定（用户 2026-11-09）：dirident 行的种类标记、护符价格、流派名；按部位生成的都是那个部位，按流派生成的词条都是那个流派
    #[test]
    fn directed_armor_talisman() {
        let items = parse_items(
            "container\tw\t240000000\t10000\t3000\t664\t96:4\ncontainer\tp\t30000000\t100\t6000\t416\t\n\
             container\ta\t20000\t1\t600\t96\t\ntext\tN\t192\ntext\tI\t160\ntext\tC\t896\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t0\t0\t8000\n\
             deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\ninherit\t93674\t93675\t93676\t9241\t93091\t93092\n\
             dirident\t93634\t93635\t6\t93641\t3\t93644\t10,100,500,1000,2000\t1,3,5\t93668\t2\t100,1000,5000,10000,20000\t异常,法术\n",
        )
        .unwrap();
        let di = items.dirid.clone().unwrap();
        assert_eq!((di.kind0, di.cost_of(2, 1), di.cost_of(1, 1), di.cost_of(0, 4)), ((93668, 2), 1000, 100, 2000));
        assert_eq!(di.archs, vec!["异常".to_string(), "法术".to_string()]);
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\affixes.txt（先跑一次重随），跳过");
            return;
        };
        assert!(lib.arch_names.len() >= 10 && lib.fam_arch.len() == lib.fams.len(), "流派 {:?}", lib.arch_names);
        let d = Defs { items, lib };
        let mut rng = Rng(5);
        for tier in 0..5 {
            for slot in 0..DIR_SLOTS as u8 {
                for _ in 0..30 {
                    let Some(Recipe::P { base, .. }) = gen_armor_slot(&d.lib, tier, slot, &mut rng) else { panic!("档 {tier} 部位 {slot} 生成不了") };
                    assert_eq!(d.lib.pbase.iter().find(|b| b.id == base).map(|b| b.slot), Some(slot));
                }
            }
            for a in 0..d.lib.arch_names.len() as u8 {
                for _ in 0..20 {
                    let Some(Recipe::A { key, .. }) = gen_talisman_arch(&d.lib, tier, a, &mut rng) else {
                        panic!("档 {tier} 流派 {} 生成不了", d.lib.arch_names[a as usize])
                    };
                    assert_eq!(d.lib.fam_arch[d.lib.fam_by_key[&key]], a, "流派不对：{key}");
                }
            }
        }
        // 菜单的种类、序号 → 请求；对不上的不要
        let di = d.items.dirid.as_ref().unwrap();
        let b = [10; 5];
        assert_eq!(dir_req(&d, di, 0, 1, 2, b), Some(DirReq { cat: 0, bias: Bias::Typed(3, b), rar: 2 }));
        assert_eq!(dir_req(&d, di, 1, 3, 5, b), Some(DirReq { cat: 1, bias: Bias::Slot(3), rar: 5 }));
        assert_eq!(dir_req(&d, di, 1, 4, 0, b), None);
        let fa = d.lib.arch_names.iter().position(|a| a == "法术").unwrap() as u8;
        assert_eq!(dir_req(&d, di, 2, 1, 3, b), Some(DirReq { cat: 2, bias: Bias::Arch(fa), rar: 3 }));
        assert_eq!(dir_req(&d, di, 2, 2, 3, b), None);
        assert_eq!(dir_req(&d, di, 3, 0, 0, b), None);
    }

    /// 武器重铸石：只换补正、需求、补正曲线，别的不变；照鉴定的规则（需求按补正排、没补正的项没需求、等级下限），
    /// 偏向加点（同一把重铸很多次，主补正落在加得最多的属性上的比例比不偏向时高）
    #[test]
    fn reforge_from_real_library() {
        let Some((_items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\affixes.txt（先跑一次重随），跳过");
            return;
        };
        let mut rng = Rng(23);
        let mut omni = 0;
        for tier in 0..5 {
            let r = tier.min(3);
            let mut changed = 0;
            for _ in 0..300 {
                let w = gen_weapon(&lib, tier, &mut rng).unwrap();
                let stats = [10 + rng.below(50) as u32, 10 + rng.below(50) as u32, 10 + rng.below(50) as u32, 10 + rng.below(50) as u32, 10];
                for s in [Some(stats), None] {
                    let n = reforge_recipe(&lib, tier, &w, s, &mut rng).unwrap();
                    let (Recipe::W { base, var, skill, st, res, glow, spec, atk, cor: c0, .. }, Recipe::W { base: b2, var: v2, skill: k2, st: s2, res: r2, glow: g2, spec: p2, atk: a2, cor, req, .. }) = (&w, &n) else {
                        panic!("不是武器")
                    };
                    assert_eq!((base, var, skill, st, res, glow, spec, atk), (b2, v2, k2, s2, r2, g2, p2, a2), "重铸改了补正以外的东西");
                    if lib.wreq_sorted {
                        assert!(ordered(cor, req), "档 {tier} 需求和补正排序不同：{cor:?} / {req:?}");
                    }
                    assert!((0..5).all(|k| cor[k] > 0.0 || req[k] == 0), "没补正的项有需求：{cor:?} / {req:?}");
                    assert!(need_level(req) >= lib.wreq_minlvl[r], "档 {tier} 等级不够：{req:?}");
                    if c0.iter().all(|&c| c > 0.0) {
                        omni += 1;
                        assert!(cor.iter().all(|&c| c > 0.0), "全能重铸后不是全能了：{c0:?} → {cor:?}");
                        let d = lib.wscal_v.get(&(r, 5)).expect("全能要有五项的补正值分布");
                        let (lo, hi) = (d.items.iter().map(|x| x.0).fold(f64::MAX, f64::min), d.items.iter().map(|x| x.0).fold(0f64, f64::max));
                        assert!(cor.iter().all(|&c| (lo..=hi).contains(&(c as f64))), "档 {tier} 全能的补正超出五项的分布 {lo}–{hi}：{cor:?}");
                    }
                    changed += (c0 != cor) as u32;
                }
            }
            assert!(changed > 500, "档 {tier} 重铸几乎没变：{changed} / 600");
        }
        assert!(omni > 20, "全能武器太少，没测到全能重铸：{omni}");
        // 偏向加点：智力加满、别的不加，主补正在智力上的比例
        let w = gen_weapon(&lib, 3, &mut rng).unwrap();
        let int_top = |s: Option<[u32; 5]>, rng: &mut Rng| {
            (0..2000)
                .filter(|_| match reforge_recipe(&lib, 3, &w, s, rng) {
                    Some(Recipe::W { cor, .. }) => cor[2] > 0.0 && (0..5).all(|k| cor[k] <= cor[2]),
                    _ => false,
                })
                .count()
        };
        let (biased, plain) = (int_top(Some([10, 10, 80, 10, 10]), &mut rng), int_top(None, &mut rng));
        if lib.bias_n > 1 {
            assert!(biased > plain, "偏向加点没起作用：{biased} / {plain}");
        }
    }

    /// 拿到时按属性生成：主补正在最高的属性上，能力需求不超过属性，战技 / 常驻效果照本体；同样的属性结果一样
    #[test]
    fn pick_recipe_follows_stats() {
        let Some((_items, lib)) = real_defs() else { return };
        let Some(b) = lib.wbase.iter().find(|b| b.id == 17030000).or_else(|| lib.wbase.first()) else { return };
        let s = StartSpec { cat: 0, id: 238000000, tier: 3, base: b.id, seed: 7, maxreq: [99; 5], top: 0.3, min_status: 0, keep: true, main: -1, pick: true, full: false };
        for stats in [[40u32, 12, 9, 10, 8], [12, 45, 10, 9, 13], [10, 10, 60, 12, 9], [11, 9, 10, 38, 20], [9, 10, 9, 8, 50], [80, 80, 10, 10, 10]] {
            let Some(Recipe::W { cor, req, skill, res, .. }) = pick_recipe(&lib, &s, stats) else { panic!("{stats:?} 生成失败") };
            let hi = stats.iter().position(|v| v == stats.iter().max().unwrap()).unwrap();
            assert_eq!(cor[hi], cor.iter().cloned().fold(f32::MIN, f32::max), "{stats:?} 主补正不在最高的属性上：{cor:?}");
            assert!((0..5).all(|k| req[k] as u32 <= stats[k]), "{stats:?} 能力需求超过属性：{req:?}");
            assert_eq!((skill, res), (-1, [KEEP_RES; 2]), "战技 / 常驻效果要照本体");
            assert_eq!(pick_recipe(&lib, &s, stats), pick_recipe(&lib, &s, stats));
        }
    }

    /// 武器商店：指定类型生成的本体都是这个类型，各档都生成得出来
    #[test]
    fn typed_weapon_from_real_library() {
        let Some((_items, lib)) = real_defs() else { return };
        let mut rng = Rng(7);
        let types: HashSet<u16> = lib.wbase.iter().map(|b| b.typ).collect();
        for &t in &types {
            for tier in 0..3 {
                let Some(Recipe::W { base, .. }) = gen_weapon_typed(&lib, tier, t, &mut rng) else { panic!("类型 {t} 档 {tier} 生成失败") };
                assert_eq!(lib.wbase.iter().find(|b| b.id == base).map(|b| b.typ), Some(t));
            }
        }
    }

    #[test]
    fn generate_from_real_library() {
        let Some((items, lib)) = real_defs() else {
            eprintln!("没有 dll\\dark_core\\items.txt / affixes.txt（先跑一次重随），跳过");
            return;
        };
        assert_eq!(items.cont[0].id(3), 240030000);
        assert_eq!(items.cont[0].idx_of(240030000), Some(3));
        assert_eq!(items.cont[0].idx_of(240030100), None);
        let mut rng = Rng(42);
        let mut over = 0;
        let mut longest: HashMap<(usize, usize, usize), usize> = HashMap::new();
        let mut seen_glow = 0;
        let mut seen_atk = 0;
        let mut skills = HashSet::new();
        for cat in 0..NCAT {
            for tier in 0..5 {
                for k in 0..400 {
                    let recipe = generate(&lib, cat, tier, &mut rng).unwrap_or_else(|| panic!("类 {cat} 档 {tier} 生成失败"));
                    let rec = Rec { cat, idx: k, tier: tier as u8, recipe };
                    assert_eq!(rec_parse(&rec_line(&rec)).as_ref(), Some(&rec));
                    match &rec.recipe {
                        Recipe::W { base, cor, st, glow, skill, atk, .. } => {
                            assert!(lib.wbase.iter().any(|b| b.id == *base));
                            assert!(cor.iter().filter(|&&c| c > 0.0).count() >= 1);
                            assert!(st.iter().all(|&s| s == -1 || s > 0));
                            assert!(*atk == 0 || *atk == lib.wassault[tier.min(3)].1, "强攻 {atk} 不是这档的数");
                            seen_glow += (*glow > 0) as i32;
                            seen_atk += (*atk > 0) as i32;
                            skills.insert(*skill);
                        }
                        Recipe::P { base, sps, .. } => {
                            assert!(lib.pbase.iter().any(|b| b.id == *base));
                            assert!((1..=3).contains(&sps.len()));
                            assert_eq!(sps.iter().map(|s| &s.1).collect::<HashSet<_>>().len(), sps.len());
                        }
                        Recipe::A { tal, .. } => assert!(*tal >= 9000 || (4141..=4144).contains(tal), "护符模板 {tal}（9000 起，疫病锁定在 4141~4144）"),
                    }
                    for lang in 0..2 {
                        let t = compose_cap(&lib, &rec, lang, items.cap[1]);
                        assert!(!t[0].is_empty());
                        for f in 0..3 {
                            let n = t[f].encode_utf16().count();
                            let m = longest.entry((cat, f, lang)).or_insert(0);
                            *m = (*m).max(n);
                            if n > items.cap[f] {
                                over += 1;
                            }
                        }
                    }
                    let row = build_row(&lib, &rec, &vec![0u8; items.cont[cat].size], false);
                    assert_eq!(row.len(), items.cont[cat].size);
                    if let Recipe::A { sp, key, .. } = &rec.recipe {
                        let want = if is_val(&lib, key, sp_rar(&lib, key, *sp).unwrap_or(0)) { -1 } else { 0 };
                        assert_eq!(get_i32(&lib, 'a', &row, "refId"), Some(want), "护符 {key} 的 refId（随机词条不挂，固定的照模板）");
                        assert!(sp_rar(&lib, key, *sp).is_some(), "护符 {key} 的效果 {sp} 不在词条库里");
                    }
                    if let Recipe::P { sps, .. } = &rec.recipe {
                        assert!(sps.iter().all(|(sp, key)| sp_rar(&lib, key, *sp).is_some()), "防具词条的效果不在词条库里：{sps:?}");
                    }
                }
            }
        }
        let mut lv: Vec<_> = longest.into_iter().collect();
        lv.sort();
        eprintln!("最长的文字（类, 字段, 语言）→ 字数：{lv:?}；占位长度 {:?}", items.cap);
        assert!(seen_glow > 0, "传说武器一把光效都没有");
        assert!(lib.wassault.iter().all(|x| x.1 == 0) || seen_atk > 0, "配了强攻却一把都没出");
        eprintln!("强攻：2000 把武器里 {seen_atk} 把");
        assert!(skills.len() > 20);
        // 超长的会被截断，不该有
        assert!(over == 0, "超出占位长度的文字 {over} 段");
        // 最坏情况：防具上最长的一条机制类 + 两条数值类（继承以后每条的稀有度可以各不相同，每条取它最长的稀有度）上了颜色，
        // 也不超过说明的占位长度
        for lang in 0..2 {
            let longest = |mech: bool| {
                let mut v: Vec<usize> = lib
                    .fams
                    .iter()
                    .filter(|f| f.mech == mech)
                    .map(|f| {
                        // 词条数值随机：每档取范围两头的说明里最长的
                        let mut m = 0;
                        for r in 0..4 {
                            let Some(fr) = f.r[r].as_ref() else { continue };
                            let mut ts = vec![fr.text[lang].clone()];
                            if let Some(v) = fr.val.as_ref() {
                                ts.extend([v.text(-1.0, lang), v.text(1.0, lang)]);
                            }
                            for t in &ts {
                                m = m.max(paint(&lib, r, t).encode_utf16().count());
                            }
                        }
                        m
                    })
                    .collect();
                v.sort_unstable_by(|a, b| b.cmp(a));
                v
            };
            let (m, n) = (longest(true), longest(false));
            let worst = m[0] + n[0] + n[1] + 2;
            eprintln!("语言 {lang}：防具说明最坏 {worst} 字（机制 {}、数值 {} + {}），占位 {}", m[0], n[0], n[1], items.cap[2]);
            assert!(worst <= items.cap[2], "语言 {lang}：防具说明最坏 {worst} 字，超过占位 {}", items.cap[2]);
        }
        let w = Rec { cat: 0, idx: 0, tier: 3, recipe: generate(&lib, 0, 3, &mut rng).unwrap() };
        eprintln!("例：{:?}", compose_cap(&lib, &w, 0, items.cap[1]));
        let p = Rec { cat: 1, idx: 0, tier: 4, recipe: generate(&lib, 1, 4, &mut rng).unwrap() };
        eprintln!("例：{:?}", compose_cap(&lib, &p, 0, items.cap[1]));
        eprintln!("例：{:?}", compose_cap(&lib, &p, 1, items.cap[1]));
        for cat in [1, 1, 1, 1, 1, 1, 0, 0, 0, 0] {
            let x = Rec { cat, idx: 0, tier: 4, recipe: generate(&lib, cat, 4, &mut rng).unwrap() };
            eprintln!("名字例：{} | {}", compose_cap(&lib, &x, 0, items.cap[1])[0], compose_cap(&lib, &x, 1, items.cap[1])[0]);
        }
    }

    /// 词条数值随机（famval 等行）：记录里的位置读写；真的词条库读得进来；生成的位置都在范围里、说明是精确的数、
    /// 装备行上不挂随机词条的效果；超限升档位置不变
    #[test]
    fn affix_value_ranges() {
        let p = Rec { cat: 1, idx: 3, tier: 2, recipe: Recipe::P { base: 40000, sps: vec![(1, "a".into()), (2, "b".into())], mult: None, pos: vec![0, -437] } };
        assert_eq!(rec_line(&p), "p\t3\t2\t40000\t1:a,2:b:-437");
        assert_eq!(rec_parse(&rec_line(&p)), Some(p));
        let old = rec_parse("p\t3\t2\t40000\t1:a,2:b").unwrap();
        assert!(matches!(&old.recipe, Recipe::P { pos, .. } if pos.is_empty()), "老记录没有位置");
        let a = Rec { cat: 2, idx: 4, tier: 1, recipe: Recipe::A { tal: 9001, sp: 7, key: "a".into(), pos: 812 } };
        assert_eq!(rec_line(&a), "a\t4\t1\t9001\t7\ta\t812");
        assert_eq!(rec_parse(&rec_line(&a)), Some(a));
        assert!(matches!(rec_parse("a\t4\t1\t9001\t7\ta").unwrap().recipe, Recipe::A { pos: 0, .. }));
        // 真的词条库
        let Some((items, lib)) = real_defs() else { return };
        let vals: Vec<(&Fam, usize, &Val)> =
            lib.fams.iter().flat_map(|f| f.r.iter().enumerate().filter_map(move |(r, x)| x.as_ref()?.val.as_deref().map(|v| (f, r, v)))).collect();
        if vals.is_empty() {
            return; // 旧的 affixes.txt（还没重随）
        }
        assert!(lib.valpool.len() >= 24 * 4, "占位行 {}", lib.valpool.len());
        let mut rng = Rng(5);
        for (f, r, v) in &vals {
            assert_eq!(v.text(0.0, 0), f.r[*r].as_ref().unwrap().text[0], "{} 稀有度 {r}：现在的数的说明和原来一样", f.key);
            for _ in 0..200 {
                let x = v.main_at(pos_f(v.sample(&mut rng)));
                assert!(x >= round_to(v.lo, v.prec) - 1e-4 && x <= round_to(v.hi, v.prec) + 1e-4, "{} 稀有度 {r}：{x} 不在 {}~{}", f.key, v.lo, v.hi);
            }
            assert!(!v.text(1.0, 0).contains('{') && !v.text(-1.0, 1).contains('{'), "{} 的说明模板没填完", f.key);
            assert!(!v.span[0].is_empty() && v.rows.len() <= 4, "{}", f.key);
        }
        eprintln!("词条数值随机：{} 档有取值，例：{} → {} / {}", vals.len(), vals[0].0.key, vals[0].2.text(-1.0, 0), vals[0].2.text(1.0, 0));
        // 生成的：随机词条有位置、装备行不挂
        let mut seen = 0;
        for _ in 0..3000 {
            let Some(rec) = gen_armor(&lib, 3, &mut rng).map(|recipe| Rec { cat: 1, idx: 0, tier: 3, recipe }) else { continue };
            let Recipe::P { sps, pos, .. } = &rec.recipe else { continue };
            let row = build_row(&lib, &rec, &vec![0u8; items.cont[1].size], false);
            for (k, (sp, key)) in sps.iter().enumerate() {
                let res = get_i32(&lib, 'p', &row, ["residentSpEffectId", "residentSpEffectId2", "residentSpEffectId3"][k]);
                if is_val(&lib, key, 3) {
                    assert_eq!(res, Some(-1), "随机词条 {key} 不挂在装备行上");
                    seen += (pos_at(pos, k) != 0) as usize;
                } else {
                    // 固定数值的、二期的（积木规则 / 模块：装备行照旧挂标记，位置由积木引擎 / 模块按身上的取）
                    assert_eq!(res, Some(*sp));
                    if !has_val(&lib, key, 3) {
                        assert_eq!(pos_at(pos, k), 0);
                    }
                }
            }
        }
        assert!(seen > 100, "随机词条抽到的位置几乎都是 0：{seen}");
        // 当场同步身上效果（继承 / 超限升级正穿着的）只管装备行上挂的：随机词条归 valfx
        let fixed = lib.fams.iter().find(|f| f.r[3].as_ref().is_some_and(|x| x.val.is_none() && x.tal >= 0)).unwrap();
        let (vf, _, _) = vals.iter().find(|(_, r, _)| *r == 3).unwrap();
        let both = Rec { cat: 1, idx: 0, tier: 3, recipe: Recipe::P {
            base: lib.pbase[0].id,
            sps: vec![(fixed.r[3].as_ref().unwrap().sp, fixed.key.clone()), (vf.r[3].as_ref().unwrap().sp, vf.key.clone())],
            mult: None,
            pos: vec![0, 300],
        } };
        assert_eq!(native_sps(&lib, &both), vec![fixed.r[3].as_ref().unwrap().sp]);
        // 超限升档：位置不变，效果换成上一档的
        let (f, _, _) = vals.iter().find(|(f, r, _)| *r == 0 && f.r[1].as_ref().is_some_and(|x| x.val.is_some())).unwrap();
        let rec = Rec { cat: 1, idx: 0, tier: 0, recipe: Recipe::P { base: lib.pbase[0].id, sps: vec![(f.r[0].as_ref().unwrap().sp, f.key.clone())], mult: None, pos: vec![-600] } };
        let (up, from, to) = overlimit::affix_up(&lib, &rec, 0).unwrap();
        assert_eq!((from, to), (0, 1));
        assert!(matches!(&up.recipe, Recipe::P { sps, pos, .. } if sps[0].0 == f.r[1].as_ref().unwrap().sp && pos == &vec![-600]));
    }
}
