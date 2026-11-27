//! 装备工坊：dll\dark_ui（悬浮界面）和 dark_core 之间的接口（用户 2026-10-31：界面单独一个 DLL、做好隔离）。
//! 界面只通过下面几个导出函数要数据、提请求，不碰游戏内存；数据在游戏线程（dark_core 的 tick）里生成，执行也在游戏线程里做
//! （规则和执行在 gear.rs 的「装备继承」那节）。
//! 赐福「装备工坊」（sc/inherit.py）点亮打开请求 open → tick 看到、界面在（最近 3 秒问过 dr_ws_state）→ 点亮应答 ack、生成快照；
//! 界面不在 → 熄灭 open、点亮 noui（菜单提示没加载 dark_ui）。界面关窗口（dr_ws_close）、3 秒没来问、菜单那边熄灭了 open →
//! 熄灭 open / ack（菜单回到赐福）。
//! 「魇印」「七咒」（用户 2026-11-03：不放进装备工坊，赐福「暗黑重铸」里各自一项，sc/wspages.py）：各自一个打开请求（Inherit.pages），
//! 应答 / 没界面和工坊共用；dr_ws_state 第 1~2 位告诉界面是哪个入口（0 装备工坊、1 魇印、2 七咒），界面只画那一页。
//! 快照（dr_ws_get(0)）一行一条、Tab 分隔、UTF-8，说明里的换行写成 \n：
//!   v 2 / lang 0|1（游戏语言：中文 / 英文） / stones N（继承消耗的融锻之心，背包里有几个） / stone_icon <融锻之心的图标>
//!   src <序号> <稀有度> <部位> <图标> <名字>                          能当来源的（没穿着、没锁定）
//!   tgt <序号> <稀有度> <部位> <穿着 0|1> <锁定 0|1> <图标> <名字>      能当目标的（全部生成防具）
//!   aff <序号> <第 i 条> <机制 0|1> <这条的稀有度> <短名> <说明>        各件的词条（来源、目标都在里面）
//!   opt <来源> <k> <目标> <替换第几条，-1 = 补上> <能 1|0> <不能的原因>
//!   nopt <来源> <k> <目标> <原因>                                 整件放不了
//!   tal <来源> <k> <能 1|0> <图标，不能时 0> <护符名字 / 原因>          提取成护符
//! 图标是游戏参数里这一行现在的图标（带稀有度边框的那份；防具按人物性别取男 / 女版），界面从菜单包的图集里画。
//! 「重铸」页（用户 2026-10-31：武器重铸集成进工坊）用的：
//!   rstones N / rstone_icon <图标>                                   武器重铸石
//!   pst <力 灵 智 信 感>                                            人物现在的属性（需求够不够、攻击力按它算）
//!   wpn <配方序号> <稀有度> <图标> <装备着 0|1> <锁定 0|1> <强化等级> <wepType> <名字>   背包里的生成武器
//!   watk <序号> <物理 基础 加成> <魔力 …> <火 …> <雷 …> <圣 …> <致命一击>   攻击力（照原版算，按现在的属性；见 weapon_info）
//!   wgrd <序号> <物理 魔力 火 雷 圣 防御时减伤> <防御强度>
//!   wcor <序号> <力 灵 智 信 感 补正（当前强化等级，0 = 没有）> / wreq <序号> <五项能力需求>
//!   winf <序号> <重量> <战技名> / wfx <序号> <附加效果一行>（说明里那几行：强攻、附加异常、特效）
//! 重铸后的范围（dr_ws_preview(序号) 要，dr_ws_get(2) 取）：按现在的加点抽 PREVIEW_N 次 reforge_recipe 统计——
//!   pv <序号> / pvn <有 1..5 项补正的百分比> / pvs <属性 0..4> <有补正的百分比> <补正最小> <最大> <需求最小> <最大>（补正按当前强化等级）
//! 「超限」页（用户 2026-11-03，超限锻造石，规则在 overlimit.rs）用的：
//!   ostones <武器石> <防具石> / ostone_icon <武器石图标> <防具石图标>
//!   wovl <序号> <超限过 0|1> <到上限还要几颗>                         每把生成武器（重铸页也用：超限过的不能重铸）
//!   wocap <序号> <属性 0..4> <现在> <中位> <原上限> <超限上限> <每颗>      有补正的每项，按当前强化等级和法器倍率换算成面板上的数
//!   aovl <序号> <现在倍率> <中位> <原上限> <超限上限> <每颗> <到上限还要几颗> <超过原上限 0|1>   每件生成防具（tgt 行那些）
//!   aup <序号> <第 i 条> <能 1|0> <升到的稀有度> <要几颗防具石> <升级后的说明 / 不能的原因>   防具词条升级（用户 2026-11-04，dr_ws_affix）
//!   acut <序号> <八种减伤：无属性 斩 打 突 魔 火 雷 圣>                本体原版的减伤比例（0.15 = 15%），界面 × 倍率（最多 90%）
//! 「魇印」页（用户 2026-11-03，魇化精英；规则在 nightmare.rs）用的：
//!   nmo <开了几个刻印位> <各级要的次数，逗号分隔> <各刻印位要解锁几种，逗号分隔>
//!   nmf <序号> <颜色 0~3> <夺取次数> <等级 0~4> <下一级要几次，0 = 满级> <名字> <现在这一级的说明> <下一级的说明> <图标>
//!   nms <刻印位> <刻的第几种，-1 = 空>  /  nme <第几种> <稀有度> <图标>（身上的魇火）
//!   图标 = 这条词条的护符模板在那个稀有度的图标（带稀有度边框；没解锁的按普通）
//! 「七咒」页（用户 2026-11-03；规则在 curses.rs）：cur <序号> <碎片> <要几片> <反转 0|1> <名字> <区域> <诅咒说明> <祝福说明>
//!   cshard <诅咒碎片图标> <祝福碎片图标> <没拿到的图标>（菜单包的自制图标 curse_shard / bless_shard / shard_empty，没有是 0）
//!   cur 行最后两列：来历、这道咒痕现在这一面的图标（自制图标 curse_<键> / bless_<键>，没有是 0）
//! （以前还有提示 dr_ws_get(3)：2026-11-03 改成原版闪烁提示，见 notes.rs）
//! 结果（dr_ws_get(1)）：ok|err <Tab> <文字>。执行完自动重新生成快照
//! 按需加载（用户 2026-11-09「不要一次性把数据都加载完 只加载前后各留一点余量」）：快照 dr_ws_get(0) 里防具、武器只有索引——
//!   a <序号> <稀有度> <部位> <穿着 0|1> <锁定 0|1> <有超限数据 0|1> <获得顺序> <词条键,…（- = 没有）> <减伤率 %> <重量>
//!     减伤率 = 八种减伤（无属性 斩 打 突 魔 火 雷 圣）的平均，重量照参数；都是游戏参数里这件现在的行（面板上的数），继承页排序用（用户 2026-11-10）
//!   w <序号> <稀有度> <装备着 0|1> <锁定 0|1> <wepType> <有超限数据 0|1> <获得顺序> <特效键（- = 没有）>
//! 上面那些明细行（tgt aff aup aovl acut tal / wpn watk … wfx / opt nopt）界面用 dr_ws_want 要看得见的行 ± 余量，游戏线程算好攒在 dr_ws_get(3)，取走就清空。
//! 要的格式一行一种：a <防具序号,…> / w <武器序号,…> / o <来源> <第 k 条> <目标序号,…> / s（关键词搜索用的文字，第一次搜索时才要）
//! 搜索文字分几帧算完（SEARCH_PER_TICK），一样从 get(3) 取：sa <防具序号> <文字> / sw <武器序号> <文字>，全部发完一行 sdone。文字里 \x01 前是名字、后是说明（拼音首字母只认名字）
use std::sync::Mutex;
use std::time::Duration;

use super::*;

/// 接口版本（dr_ws_state 的高 16 位，界面对不上就不用）：2 = 快照带图标（2026-10-31 界面照原版商店重做）；3 = 加「重铸」页；
/// 4 = 加「超限」页（dr_ws_overlimit、快照 ostones / wovl / wocap / aovl / acut）；5 = 加「魇印」页（dr_ws_seal、快照 nm*、提示 dr_ws_get(3)）；
/// 6 = 加「七咒」页（快照 cur）；7 = 「魇印」「七咒」各自从赐福打开（dr_ws_state 第 1~2 位 = 入口）；
/// 8 = 去掉提示 dr_ws_get(3)（改成原版闪烁提示，notes.rs）；9 = 「超限」页防具词条升级（dr_ws_affix、快照 aup）；
/// 12 = 快照只有索引，明细按需要来（dr_ws_want、dr_ws_get(3)）；13 = 索引带获得顺序和词条键，want 加 s（搜索文字）；
/// 14 = 「超限」页武器栏的弃置按钮（dr_ws_discard，快照 w 行末尾多一列弃置 0|1）
const API: u32 = 14;
/// 搜索用的文字一帧算几件（第一次搜索时才算，分几帧算完不卡）
const SEARCH_PER_TICK: usize = 32;
/// 重铸范围预览抽几次
const PREVIEW_N: usize = 1500;
/// 界面多久没来问就当它不在了
const ALIVE: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Ws {
    /// 窗口开着（菜单请求过、DLL 应答了）
    open: bool,
    /// 哪个入口打开的：0 装备工坊、1 魇印、2 七咒
    mode: u32,
    /// 界面最近一次来问的时间
    seen: Option<Instant>,
    close: bool,
    want_snap: bool,
    /// (版本, 文字)
    snap: (u32, String),
    /// 执行请求：(来源, 第 k 条, 目标（None = 提取成护符）, 替换第几条（None = 补上）)
    req: Option<(u32, usize, Option<u32>, Option<usize>)>,
    /// 重铸请求：武器的配方序号
    reforge: Option<u32>,
    /// 超限请求：(类 0 武器 / 1 防具, 配方序号, 最多用几颗)
    ovl: Option<(usize, u32, u32)>,
    /// 防具词条升级请求：(配方序号, 第 i 条)
    aup: Option<(u32, usize)>,
    /// 刻印请求：(刻印位, 第几种，None = 取下)
    seal: Option<(usize, Option<usize>)>,
    /// 弃置请求：武器的配方序号（有就切换，见 gear.rs 的 discard_apply）
    discard: Option<u32>,
    result: (u32, String),
    /// 要算重铸范围的武器、算好的 (版本, 文字)
    want_preview: Option<u32>,
    preview: (u32, String),
    /// 界面要的明细（dr_ws_want 的文字，tick 里算）、算好还没被取走的 (版本, 文字)
    want: String,
    patch: (u32, String),
    /// 搜索文字算到第几件（防具在前，再接武器），None = 没在算
    search_pos: Option<usize>,
    /// 这次快照的防具 / 武器索引（算明细时按它取，不用再扫背包、读锁定文件）
    armor: Vec<IhArmor>,
    weapons: Vec<WRow>,
}

static WS: Mutex<Option<Ws>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Ws) -> R) -> R {
    let mut g = WS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Ws::default))
}

/// 界面每帧来问：返回 接口版本 << 16 | 入口 << 1（第 1~2 位）| 窗口该不该开着（第 0 位）；同时算界面还在
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_state() -> u32 {
    with(|w| {
        w.seen = Some(Instant::now());
        (API << 16) | (w.mode << 1) | w.open as u32
    })
}

/// 界面关窗口
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_close() {
    with(|w| w.close = true)
}

/// 重新生成快照
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_refresh() {
    with(|w| w.want_snap = true)
}

/// 执行请求：target < 0 = 提取成护符；replace < 0 = 补上
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_apply(src: u32, k: u32, target: i64, replace: i32) {
    with(|w| w.req = Some((src, k as usize, (target >= 0).then_some(target as u32), (replace >= 0).then_some(replace as usize))))
}

/// 重铸请求：武器的配方序号（快照 wpn 行的第一个数）
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_reforge(k: u32) {
    with(|w| w.reforge = Some(k))
}

/// 超限请求：cat 0 武器 / 1 防具，k = 配方序号，n = 最多用几颗（0xFFFFFFFF = 用到上限）
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_overlimit(cat: u32, k: u32, n: u32) {
    with(|w| w.ovl = Some((cat as usize, k, n.max(1))))
}

/// 防具词条升级请求：k = 配方序号，i = 第几条（快照 aff 行的前两个数）
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_affix(k: u32, i: u32) {
    with(|w| w.aup = Some((k, i as usize)))
}

/// 弃置 / 取消弃置一把生成武器：k = 配方序号（快照 w 行的第一个数）。只写弃置名单，不动游戏数据
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_discard(k: u32) {
    with(|w| w.discard = Some(k))
}

/// 刻印请求：slot = 刻印位，fam = 第几种（< 0 = 取下）
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_seal(slot: u32, fam: i32) {
    with(|w| w.seal = Some((slot as usize, usize::try_from(fam).ok())))
}

/// 要这把武器重铸后的范围（算好放进 dr_ws_get(2)）
#[unsafe(no_mangle)]
pub extern "C" fn dr_ws_preview(k: u32) {
    with(|w| w.want_preview = Some(k))
}

/// 要明细（格式见文件头）：累积起来，游戏线程 tick 里算
///
/// # Safety
/// p 要能读 len 个字节
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dr_ws_want(p: *const u8, len: usize) {
    if p.is_null() {
        return;
    }
    let s = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p, len) }).into_owned();
    with(|w| w.want.push_str(&s));
}

/// which 0 = 快照、1 = 结果、2 = 重铸范围、3 = 算好的明细（取走就清空；别的当 2）：buf 够大就拷进去（不带结束符），返回 (版本 << 32) | 长度；不够大返回 -长度（版本 0 = 还没有）
///
/// # Safety
/// buf 要能写 cap 个字节
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dr_ws_get(which: u32, buf: *mut u8, cap: usize) -> i64 {
    with(|w| {
        let (ver, s) = match which {
            0 => (&w.snap.0, &w.snap.1),
            1 => (&w.result.0, &w.result.1),
            3 => (&w.patch.0, &w.patch.1),
            _ => (&w.preview.0, &w.preview.1),
        };
        let b = s.as_bytes();
        if buf.is_null() || b.len() > cap {
            return -(b.len() as i64);
        }
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), buf, b.len()) };
        let r = ((*ver as i64) << 32) | b.len() as i64;
        if which == 3 {
            w.patch.1.clear();
        }
        r
    })
}

/// 游戏语言：游戏取到的装备文字占位多数是哪种（0 中文 / 1 英文；msghook.rs 数的）
fn ui_lang() -> usize {
    crate::msghook::ui_lang()
}

pub(super) fn tick(d: &Defs, st: &mut St, pgd: &mut PlayerGameData, who: &(i32, String)) {
    let Some(ih) = d.items.inherit else { return };
    // 三个入口的打开请求：装备工坊、魇印、七咒（0 = 没有这个入口）
    let opens = [ih.open, ih.pages[0], ih.pages[1]];
    let lit = |k: usize| opens[k] != 0 && flag_get(opens[k]) == Some(true);
    const NAMES: [&str; 3] = ["装备工坊", "魇印", "咒痕"];
    let (open, mode, alive, close) = with(|w| (w.open, w.mode as usize, w.seen.is_some_and(|t| t.elapsed() < ALIVE), std::mem::take(&mut w.close)));
    if !open {
        let Some(want) = (0..3).find(|&k| lit(k)) else { return };
        if flag_get(ih.ack) != Some(true) {
            if st.frames < SETTLE_FRAMES {
                flag_set(opens[want], false);
                log(&format!("{}：进存档时打开请求亮着（存档里残留的），已熄灭", NAMES[want]));
            } else if alive {
                with(|w| {
                    w.open = true;
                    w.mode = want as u32;
                    w.want_snap = true;
                    w.want.clear();
                    w.patch.1.clear();
                    w.search_pos = None;
                    w.req = None;
                    w.reforge = None;
                    w.ovl = None;
                    w.aup = None;
                    w.seal = None;
                    w.discard = None;
                });
                flag_set(ih.ack, true);
                log(&format!("{}：打开", NAMES[want]));
            } else {
                flag_set(opens[want], false);
                flag_set(ih.noui, true);
                log(&format!("{}：没有界面（dll\\dark_ui 没加载或没初始化好），菜单提示", NAMES[want]));
            }
        }
        return;
    }
    if close || !alive || !lit(mode) {
        with(|w| {
            w.open = false;
            w.want.clear();
            w.patch.1.clear();
            w.search_pos = None;
            w.req = None;
            w.reforge = None;
            w.ovl = None;
            w.aup = None;
            w.seal = None;
            w.discard = None;
        });
        flag_set(opens[mode], false);
        flag_set(ih.ack, false);
        log(&format!("{}：关闭{}", NAMES[mode], if close { "" } else if !alive { "（界面 3 秒没来问）" } else { "（菜单那边关了）" }));
        return;
    }
    let (req, reforge, ovl, aup, seal, disc) = with(|w| (w.req.take(), w.reforge.take(), w.ovl.take(), w.aup.take(), w.seal.take(), w.discard.take()));
    if let Some((slot, fam)) = seal {
        // 刻印不改装备，不要求坐在赐福点
        let res = crate::nightmare::set_slot(slot, fam);
        let lang = ui_lang();
        let text = match &res {
            Ok(m) => format!("ok\t{}", m[lang]),
            Err(m) => format!("err\t{}", m[lang]),
        };
        with(|w| {
            w.result = (w.result.0 + 1, text);
            w.want_snap = true;
        });
    }
    if let Some(k) = disc {
        // 弃置只写名单、不动游戏数据；工坊只能从赐福打开，at_grace 当第二道防线
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !at_grace() {
                Err(["人物不在赐福点".to_string(), "You are not resting at a site of grace.".to_string()])
            } else {
                discard_apply(d, st, pgd, who, k)
            }
        }));
        let res = run.unwrap_or_else(|_| {
            log("弃置：执行时出错（详情见上一行 panic），名单可能没改");
            Err([
                "出错了，这次没有做完。关掉再打开看看".to_string(),
                "Something went wrong and this was not finished. Reopen to see the current state.".to_string(),
            ])
        });
        let lang = ui_lang();
        let text = match &res {
            Ok(m) => format!("ok\t{}", m[lang]),
            Err(m) => format!("err\t{}", m[lang]),
        };
        if res.is_ok() {
            crate::clash::workshop_sound();
        }
        log(&format!("装备工坊：{}", text.replace('\t', " ")));
        with(|w| {
            w.result = (w.result.0 + 1, text);
            w.want_snap = true;
        });
    }
    if req.is_some() || reforge.is_some() || ovl.is_some() || aup.is_some() {
        // 执行时出错（panic）也要回界面一句，不然界面一直等在「处理中」（2026-11-19 玩家诊断：扣融锻之心越界，继承卡住）；
        // 出错前改了多少记录就存多少，让记录和背包对得上
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !at_grace() {
                Err(["人物不在赐福点".to_string(), "You are not resting at a site of grace.".to_string()])
            } else if let Some((src, k, tgt, rep)) = req {
                ih_apply(d, st, pgd, who, src, k, tgt, rep)
            } else if let Some((k, i)) = aup {
                overlimit::affix_apply(d, st, pgd, who, k, i)
            } else if let Some((cat, k, n)) = ovl {
                let r = overlimit::apply(d, st, pgd, who, cat, k, n);
                if r.is_ok() && cat == 0 {
                    with(|w| w.want_preview = Some(k));
                }
                r
            } else {
                let k = reforge.unwrap_or_default();
                let r = reforge_apply(d, st, pgd, who, k);
                if r.is_ok() {
                    with(|w| w.want_preview = Some(k)); // 补正等级变了（全能的名字也变了），范围按新的重算一次
                }
                r
            }
        }));
        let res = run.unwrap_or_else(|_| {
            log(&format!("{}：执行时出错（详情见上一行 panic），已存记录、告诉界面", NAMES[mode]));
            rec_save(who, st);
            Err([
                "出错了，这次没有做完。关掉再打开看看装备现在的样子".to_string(),
                "Something went wrong and this was not finished. Reopen to see the current state of your gear.".to_string(),
            ])
        });
        let lang = ui_lang();
        let text = match &res {
            Ok(m) => format!("ok\t{}", m[lang]),
            Err(m) => format!("err\t{}", m[lang]),
        };
        // 成功了放一声原版强化武器的音效（用户 2026-11-10；clash.rs 的 usound 标记 → 事件脚本放）
        if res.is_ok() {
            crate::clash::workshop_sound();
        }
        log(&format!("装备工坊：{}", text.replace('\t', " ")));
        with(|w| {
            w.result = (w.result.0 + 1, text);
            w.want_snap = true;
        });
    }
    if with(|w| std::mem::take(&mut w.want_snap)) {
        // 顺便修剪弃置名单（用户 2026-11-26）：卖掉 / 丢掉 / 回收掉的武器留着条目会变成幽灵，
        // 按「背包和木箱里都没有了」才算真没了（只看背包会把放进木箱的武器误删，取回来时状态就丢了）
        let pruned = discard_prune(pgd, who);
        if pruned > 0 {
            log(&format!("弃置名单：清掉 {pruned} 条（背包和木箱里都没有了）"));
        }
        let (s, armor, weapons) = snapshot(d, st, pgd, mode as u32);
        with(|w| {
            w.snap = (w.snap.0 + 1, s);
            (w.armor, w.weapons) = (armor, weapons);
            // 换了快照：之前要的、算好的都作废（界面看到新快照会重新要）
            w.want.clear();
            w.patch.1.clear();
            w.search_pos = None;
        });
    }
    let want = with(|w| std::mem::take(&mut w.want));
    if !want.is_empty() {
        let (armor, weapons) = with(|w| (std::mem::take(&mut w.armor), std::mem::take(&mut w.weapons)));
        let s = details(d, st, pgd, &want, &armor, &weapons);
        with(|w| {
            (w.armor, w.weapons) = (armor, weapons);
            w.patch.0 = w.patch.0.wrapping_add(1);
            w.patch.1.push_str(&s);
        });
    }
    if let Some(pos) = with(|w| w.search_pos) {
        let (armor, weapons) = with(|w| (std::mem::take(&mut w.armor), std::mem::take(&mut w.weapons)));
        let (s, next) = search_lines(d, st, &armor, &weapons, pos);
        with(|w| {
            (w.armor, w.weapons) = (armor, weapons);
            w.search_pos = next;
            w.patch.0 = w.patch.0.wrapping_add(1);
            w.patch.1.push_str(&s);
        });
    }
    if let Some(k) = with(|w| w.want_preview.take()) {
        let s = reforge_preview(d, st, pgd, k);
        with(|w| w.preview = (w.preview.0 + 1, s));
    }
}

/// 背包里的一把生成武器（索引：快照时算一次，明细按它取）
#[derive(Clone)]
struct WRow {
    k: u32,
    pid: u32,
    rar: usize,
    worn: bool,
    locked: bool,
    /// 被玩家标了弃置（「超限」页武器栏的第 4 个按钮；只影响一键出售和武器回收）
    discard: bool,
    wtype: u16,
    /// 有超限数据（超限页列得出来）
    ovl: bool,
    /// 获得顺序（背包格子的 sort_id，越大越新）、特效的键（没有是 -）
    sort: u32,
    spec: String,
}

/// 背包里的生成武器索引：传说在前，只查参数行、不算数值
fn weapon_index(d: &Defs, st: &St, pgd: &PlayerGameData, repo: &Option<&mut SoloParamRepository>) -> Vec<WRow> {
    let (_, eq_idx) = equipped(pgd);
    let key_cap = pgd.equipment.equip_inventory_data.items_data.key_items_capacity;
    let locked = st.who.as_ref().map(|w| locked_keys(&rec_read(LOCK_FILE).unwrap_or_default(), w)).unwrap_or_default();
    let disc = st.who.as_ref().map(|w| discard_keys(&discard_text(), w)).unwrap_or_default();
    let mut list: Vec<(u32, usize, u32, usize)> = inv_weapons(d, st, pgd).into_iter().map(|(k, slot, pid)| (k, slot, pid, st.recs[&(0, k)].rar())).collect();
    list.sort_by_key(|x| (std::cmp::Reverse(x.3), x.0));
    let mut out = Vec::with_capacity(list.len());
    for (k, slot, pid, rar) in list {
        let rec = &st.recs[&(0, k)];
        let base = rec_id(&d.items, rec);
        // 和 weapon_info 用同样的两行：明细算得出来才列
        let Some(w) = repo.as_deref().and_then(|r| r.get::<EquipParamWeapon>(base).filter(|w| r.get::<ReinforceParamWeapon>(w.reinforce_type_id() as u32 + pid - base).is_some())) else { continue };
        let ovl = matches!(&rec.recipe, Recipe::W { cor, .. } if d.items.overlimit.is_some() && overlimit::cap_w(&d.lib, rar, cor).is_some() && cor.iter().any(|&c| c > 0.0));
        let sort = pgd.equipment.equip_inventory_data.items_data.normal_entries().get(slot).and_then(|e| e.as_option()).map_or(0, |e| e.sort_id);
        let spec = match &rec.recipe {
            Recipe::W { spec, .. } => d.lib.wspec.iter().find(|x| x.sp == *spec).map_or("-".to_string(), |x| x.key.clone()),
            _ => "-".to_string(),
        };
        out.push(WRow { k, pid, rar, worn: eq_idx.contains(&(key_cap + slot as u32)), locked: locked.contains(&(0, pid / 100 * 100)), discard: disc.contains(&base), wtype: w.wep_type() as u16, ovl, sort, spec });
    }
    out
}

/// 这几把武器的明细：wpn 和原版那样的数值（watk / wgrd / wcor / wreq / winf / wfx，格式见文件头）
fn weapon_lines(d: &Defs, st: &St, pgd: &PlayerGameData, repo: &mut Option<&mut SoloParamRepository>, lang: usize, rows: &[&WRow], out: &mut String) {
    let stats = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    let female = pgd.gender == 0;
    for r in rows {
        let (k, pid, rar, worn, lock) = (r.k, r.pid, r.rar, r.worn, r.locked);
        let rec = &st.recs[&(0, k)];
        let base = rec_id(&d.items, rec);
        let level = pid - base;
        let icon = param_icon(d, repo.as_deref_mut(), female, 0, base);
        let Some(info) = repo.as_deref().and_then(|r| weapon_info(r, base, level, stats)) else { continue };
        out.push_str(&format!(
            "wpn\t{k}\t{rar}\t{icon}\t{}\t{}\t{level}\t{}\t{}\n",
            worn as u8,
            lock as u8,
            info.wep_type,
            esc(&plain_name(d, rec, lang))
        ));
        let atk: Vec<String> = (0..5).flat_map(|e| [format!("{:.0}", info.base[e].floor()), format!("{:.0}", info.bonus[e].trunc())]).collect();
        out.push_str(&format!("watk\t{k}\t{}\t{:.0}\n", atk.join("\t"), info.crit));
        let g: Vec<String> = info.guard.iter().map(|x| format!("{x:.1}")).collect();
        out.push_str(&format!("wgrd\t{k}\t{}\t{:.0}\n", g.join("\t"), info.boost.floor()));
        let c: Vec<String> = info.cor.iter().map(|x| format!("{:.0}", x.floor())).collect();
        out.push_str(&format!("wcor\t{k}\t{}\n", c.join("\t")));
        let r: Vec<String> = info.req.iter().map(|x| x.to_string()).collect();
        out.push_str(&format!("wreq\t{k}\t{}\n", r.join("\t")));
        // 超限：配方里的补正 × 强化倍率 × 法器倍率 × 全能补正倍率 = 面板上的数（中位、上限、每颗也按同样换算）
        if let Recipe::W { cor, ovl, .. } = &rec.recipe {
            let need = d.items.overlimit.map_or(0, |o| overlimit::need(&d.lib, rec, &o));
            out.push_str(&format!("wovl\t{k}\t{}\t{need}\n", *ovl as u8));
            // 超限上限是这把自己的（法杖、圣印记以外的武器每把 × 1 ± wspread，overlimit::cap_rec）
            if let Some((med, hi, cap, s)) = d.items.overlimit.and_then(|o| overlimit::cap_rec(&d.lib, rec, &o)) {
                let cm = (cast_base(&d.lib, rec).map_or(1.0, |b| b / 100.0) * omni_mult(&d.lib, cor)) as f32;
                for (i, &c) in cor.iter().enumerate().filter(|x| *x.1 > 0.0) {
                    let f = info.cor_rate[i] * cm;
                    out.push_str(&format!("wocap\t{k}\t{i}\t{:.2}\t{:.2}\t{:.2}\t{:.2}\t{:.3}\n", c * f, med * f, hi * f, cap * f, s * f));
                }
            }
        }
        let skill = match &rec.recipe {
            Recipe::W { skill, .. } => d.lib.skillinfo.get(skill).map(|i| i.name[lang].clone()).unwrap_or_default(),
            _ => String::new(),
        };
        out.push_str(&format!("winf\t{k}\t{:.1}\t{}\n", info.weight, esc(&skill)));
        // 附加效果：说明里那几行（强攻、附加异常、特效），去掉颜色标记
        for l in strip_font(&compose_cap(&d.lib, rec, lang, d.items.cap[1])[2]).split('\n').filter(|l| !l.trim().is_empty()) {
            out.push_str(&format!("wfx\t{k}\t{}\n", esc(l)));
        }
    }
}

/// 原版面板上要变金色的补正（用户 2026-11-03：超过原上限之后变金色，scaling.rs 印字时比对）：背包里超限过的生成武器，
/// 补正（配方里的）超过这个稀有度原上限的那几项，按现在的强化等级算出面板上的数（同 weapon_info：参数行的补正 × 强化倍率）
pub(super) fn gold_tick(d: &Defs, st: &St, pgd: &PlayerGameData) {
    if !st.recs.iter().any(|(k, r)| k.0 == 0 && matches!(r.recipe, Recipe::W { ovl: true, .. })) {
        crate::scaling::set_gold(Vec::new());
        return;
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return };
    let mut out = Vec::new();
    for (k, _slot, pid) in inv_weapons(d, st, pgd) {
        let rec = &st.recs[&(0, k)];
        let Recipe::W { cor, ovl: true, .. } = &rec.recipe else { continue };
        let Some((_, hi, _)) = overlimit::cap_w(&d.lib, rec.rar(), cor) else { continue };
        let base = rec_id(&d.items, rec);
        let Some(info) = weapon_info(repo, base, pid - base, [99; 5]) else { continue };
        out.extend((0..5).filter(|&s| cor[s] > hi + 1e-3).map(|s| info.cor[s]));
    }
    crate::scaling::set_gold(out);
}

/// 一把武器原版那样的数值（weapon_info 算）
struct WInfo {
    /// 物理、魔力、火、雷、圣：基础值、加成（需求不够时是负的）
    base: [f32; 5],
    bonus: [f32; 5],
    crit: f32,
    guard: [f32; 5],
    boost: f32,
    /// 补正（当前强化等级）、强化给补正乘的倍率
    cor: [f32; 5],
    cor_rate: [f32; 5],
    req: [u8; 5],
    weight: f32,
    wep_type: u16,
}

type AecRow = <AttackElementCorrectParam as SoloParam>::StructType;

/// 攻击属性补正表：属性 s（力 灵 智 信 感）影响不影响元素 e（物理 魔力 火 雷 圣）、覆盖的补正（-1 = 不覆盖）、影响率（%）
fn aec(a: &AecRow, s: usize, e: usize) -> (bool, i16, i16) {
    macro_rules! r {
        ($is:ident, $ow:ident, $inf:ident) => {
            (a.$is(), a.$ow(), a.$inf())
        };
    }
    match (s, e) {
        (0, 0) => r!(is_strength_correct_by_physics, overwrite_strength_correct_rate_by_physics, influence_strength_correct_rate_by_physics),
        (1, 0) => r!(is_dexterity_correct_by_physics, overwrite_dexterity_correct_rate_by_physics, influence_dexterity_correct_rate_by_physics),
        (2, 0) => r!(is_magic_correct_by_physics, overwrite_magic_correct_rate_by_physics, influence_magic_correct_rate_by_physics),
        (3, 0) => r!(is_faith_correct_by_physics, overwrite_faith_correct_rate_by_physics, influence_faith_correct_rate_by_physics),
        (4, 0) => r!(is_luck_correct_by_physics, overwrite_luck_correct_rate_by_physics, influence_luck_correct_rate_by_physics),
        (0, 1) => r!(is_strength_correct_by_magic, overwrite_strength_correct_rate_by_magic, influence_strength_correct_rate_by_magic),
        (1, 1) => r!(is_dexterity_correct_by_magic, overwrite_dexterity_correct_rate_by_magic, influence_dexterity_correct_rate_by_magic),
        (2, 1) => r!(is_magic_correct_by_magic, overwrite_magic_correct_rate_by_magic, influence_magic_correct_rate_by_magic),
        (3, 1) => r!(is_faith_correct_by_magic, overwrite_faith_correct_rate_by_magic, influence_faith_correct_rate_by_magic),
        (4, 1) => r!(is_luck_correct_by_magic, overwrite_luck_correct_rate_by_magic, influence_luck_correct_rate_by_magic),
        (0, 2) => r!(is_strength_correct_by_fire, overwrite_strength_correct_rate_by_fire, influence_strength_correct_rate_by_fire),
        (1, 2) => r!(is_dexterity_correct_by_fire, overwrite_dexterity_correct_rate_by_fire, influence_dexterity_correct_rate_by_fire),
        (2, 2) => r!(is_magic_correct_by_fire, overwrite_magic_correct_rate_by_fire, influence_magic_correct_rate_by_fire),
        (3, 2) => r!(is_faith_correct_by_fire, overwrite_faith_correct_rate_by_fire, influence_faith_correct_rate_by_fire),
        (4, 2) => r!(is_luck_correct_by_fire, overwrite_luck_correct_rate_by_fire, influence_luck_correct_rate_by_fire),
        (0, 3) => r!(is_strength_correct_by_thunder, overwrite_strength_correct_rate_by_thunder, influence_strength_correct_rate_by_thunder),
        (1, 3) => r!(is_dexterity_correct_by_thunder, overwrite_dexterity_correct_rate_by_thunder, influence_dexterity_correct_rate_by_thunder),
        (2, 3) => r!(is_magic_correct_by_thunder, overwrite_magic_correct_rate_by_thunder, influence_magic_correct_rate_by_thunder),
        (3, 3) => r!(is_faith_correct_by_thunder, overwrite_faith_correct_rate_by_thunder, influence_faith_correct_rate_by_thunder),
        (4, 3) => r!(is_luck_correct_by_thunder, overwrite_luck_correct_rate_by_thunder, influence_luck_correct_rate_by_thunder),
        (0, 4) => r!(is_strength_correct_by_dark, overwrite_strength_correct_rate_by_dark, influence_strength_correct_rate_by_dark),
        (1, 4) => r!(is_dexterity_correct_by_dark, overwrite_dexterity_correct_rate_by_dark, influence_dexterity_correct_rate_by_dark),
        (2, 4) => r!(is_magic_correct_by_dark, overwrite_magic_correct_rate_by_dark, influence_magic_correct_rate_by_dark),
        (3, 4) => r!(is_faith_correct_by_dark, overwrite_faith_correct_rate_by_dark, influence_faith_correct_rate_by_dark),
        _ => r!(is_luck_correct_by_dark, overwrite_luck_correct_rate_by_dark, influence_luck_correct_rate_by_dark),
    }
}

/// 补正曲线（CalcCorrectGraph）在属性值 x 的值（%）：五段，每段按 adjPt 弯曲（同 rr/startgear.py 的 max_load）；procs 也用
pub(crate) fn graph(g: &<CalcCorrectGraph as SoloParam>::StructType, x: f32) -> f32 {
    let smv = [g.stage_max_val0(), g.stage_max_val1(), g.stage_max_val2(), g.stage_max_val3(), g.stage_max_val4()];
    let grow = [g.stage_max_grow_val0(), g.stage_max_grow_val1(), g.stage_max_grow_val2(), g.stage_max_grow_val3(), g.stage_max_grow_val4()];
    let adj = [g.adj_pt_max_grow_val0(), g.adj_pt_max_grow_val1(), g.adj_pt_max_grow_val2(), g.adj_pt_max_grow_val3(), g.adj_pt_max_grow_val4()];
    if x <= smv[0] {
        return grow[0];
    }
    let mut i = 0;
    while i < 3 && x > smv[i + 1] {
        i += 1;
    }
    if x >= smv[4] {
        return grow[4];
    }
    let span = smv[i + 1] - smv[i];
    let t = if span > 0.0 { ((x - smv[i]) / span).clamp(0.0, 1.0) } else { 1.0 };
    let t = match adj[i] {
        a if a > 0.0 => t.powf(a),
        a if a < 0.0 => 1.0 - (1.0 - t).powf(-a),
        _ => t,
    };
    grow[i] + (grow[i + 1] - grow[i]) * t
}

/// 武器参数行 id、强化等级、人物属性（力 灵 智 信 感）→ 原版界面那些数值。攻击力照原版算：
/// 每种元素基础值 = 武器 × 强化倍率；加成 = Σ 影响这种元素的属性（攻击属性补正表）基础值 × 补正（覆盖值或武器的 × 强化倍率）
/// × 影响率 × 补正曲线(属性值)；影响它的属性有一项没达到需求：加成 = −40% 基础值（原版需求不足的惩罚）。单手、不算增益
fn weapon_info(repo: &SoloParamRepository, id: u32, level: u32, stats: [u32; 5]) -> Option<WInfo> {
    let w = repo.get::<EquipParamWeapon>(id)?;
    let rf = repo.get::<ReinforceParamWeapon>(w.reinforce_type_id() as u32 + level)?;
    let base = [
        w.attack_base_physics() as f32 * rf.physics_atk_rate(),
        w.attack_base_magic() as f32 * rf.magic_atk_rate(),
        w.attack_base_fire() as f32 * rf.fire_atk_rate(),
        w.attack_base_thunder() as f32 * rf.thunder_atk_rate(),
        w.attack_base_dark() as f32 * rf.dark_atk_rate(),
    ];
    let cor_rate = [rf.correct_strength_rate(), rf.correct_agility_rate(), rf.correct_magic_rate(), rf.correct_faith_rate(), rf.correct_luck_rate()];
    let wcor = [w.correct_strength(), w.correct_agility(), w.correct_magic(), w.correct_faith(), w.correct_luck()];
    let cor: [f32; 5] = std::array::from_fn(|s| wcor[s] as f32 * cor_rate[s]);
    let req = [w.proper_strength(), w.proper_agility(), w.proper_magic(), w.proper_faith(), w.proper_luck()];
    let curves = [w.correct_type_physics(), w.correct_type_magic(), w.correct_type_fire(), w.correct_type_thunder(), w.correct_type_dark()];
    let a = repo.get::<AttackElementCorrectParam>(w.attack_element_correct_id() as u32);
    let mut bonus = [0f32; 5];
    for e in 0..5 {
        if base[e] <= 0.0 {
            continue;
        }
        let (Some(a), Some(g)) = (a, repo.get::<CalcCorrectGraph>(curves[e] as u32)) else { continue };
        let mut short = false;
        for s in 0..5 {
            let (on, ow, inf) = aec(a, s, e);
            if !on {
                continue;
            }
            short |= stats[s] < req[s] as u32;
            let c = if ow >= 0 { ow as f32 * cor_rate[s] } else { cor[s] };
            bonus[e] += base[e] * c / 100.0 * inf as f32 / 100.0 * graph(g, stats[s] as f32) / 100.0;
        }
        if short {
            bonus[e] = -0.4 * base[e];
        }
    }
    Some(WInfo {
        base,
        bonus,
        crit: 100.0 + w.throw_atk_rate() as f32,
        guard: [
            w.phys_guard_cut_rate() * rf.physics_guard_cut_rate(),
            w.mag_guard_cut_rate() * rf.magic_guard_cut_rate(),
            w.fire_guard_cut_rate() * rf.fire_guard_cut_rate(),
            w.thun_guard_cut_rate() * rf.thunder_guard_cut_rate(),
            w.dark_guard_cut_rate() * rf.dark_guard_cut_rate(),
        ],
        boost: w.stamina_guard_def() as f32 * rf.stamina_guard_def_rate(),
        cor,
        cor_rate,
        req,
        weight: w.weight(),
        wep_type: w.wep_type() as u16,
    })
}

/// 背包里这个角色的生成武器：(配方序号, 背包格序号, 物品编号（含强化等级）)
fn inv_weapons(d: &Defs, st: &St, pgd: &PlayerGameData) -> Vec<(u32, usize, u32)> {
    let mut out: Vec<(u32, usize, u32)> = Vec::new();
    for (i, e) in pgd.equipment.equip_inventory_data.items_data.normal_entries().iter().enumerate() {
        let Some(it) = e.as_option() else { continue };
        if it.item_id.category() as u8 != INV_CAT[0] || it.quantity == 0 {
            continue;
        }
        let pid = it.item_id.param_id();
        let base = pid / 10000 * 10000;
        let Some(k) = d.items.cont[0].idx_of(base).or_else(|| start_off(&d.items, 0, base)) else { continue };
        if st.recs.contains_key(&(0, k)) && !out.iter().any(|x| x.0 == k) {
            out.push((k, i, pid));
        }
    }
    out
}

/// 重铸后的范围：按现在的加点抽 PREVIEW_N 次 reforge_recipe，统计每项属性有补正的比例、补正（按当前强化等级，抽到全能的再乘全能补正倍率）
/// 和需求的范围、补正项数的分布（全能重铸后还是全能：只有五项）
fn reforge_preview(d: &Defs, st: &St, pgd: &PlayerGameData, k: u32) -> String {
    let Some(rec) = st.recs.get(&(0, k)) else { return String::new() };
    let base = rec_id(&d.items, rec);
    let level = inv_weapons(d, st, pgd).iter().find(|x| x.0 == k).map_or(0, |x| x.2 - base);
    // 强化倍率：按现在的强化类型和等级（全能和其他武器同一个强化类型，rr/upgrade.py，用户 2026-11-24）
    let repo = unsafe { SoloParamRepository::instance() }.ok();
    let rates = repo
        .and_then(|r| r.get::<EquipParamWeapon>(base))
        .and_then(|w| repo?.get::<ReinforceParamWeapon>((w.reinforce_type_id() as u32).wrapping_add(level)))
        .map_or([1.0; 5], |rf| [rf.correct_strength_rate(), rf.correct_agility_rate(), rf.correct_magic_rate(), rf.correct_faith_rate(), rf.correct_luck_rate()]);
    let stats = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
    // 法器：参数行的补正值乘了法器倍率（build_row），范围也乘上，和面板对得上（倍率按记录定，重铸不变）；
    // 抽到全能的再 × 全能补正倍率（omni_mult）
    let cm = cast_base(&d.lib, rec).map_or(1.0, |b| b / 100.0) as f32;
    let mut rng = Rng::new();
    let (mut n_by, mut has) = ([0u32; 6], [0u32; 5]);
    let (mut cmin, mut cmax, mut rmin, mut rmax) = ([f32::MAX; 5], [0f32; 5], [u8::MAX; 5], [0u8; 5]);
    let mut total = 0u32;
    for _ in 0..PREVIEW_N {
        let Some(Recipe::W { cor, req, .. }) = reforge_recipe(&d.lib, rec.tier as usize, &rec.recipe, Some(stats), &mut rng) else { continue };
        total += 1;
        let n = cor.iter().filter(|&&c| c > 0.0).count().min(5);
        n_by[n] += 1;
        let om = omni_mult(&d.lib, &cor) as f32;
        for s in 0..5 {
            if cor[s] > 0.0 {
                has[s] += 1;
                let c = cor[s] * rates[s] * cm * om;
                cmin[s] = cmin[s].min(c);
                cmax[s] = cmax[s].max(c);
                rmin[s] = rmin[s].min(req[s]);
                rmax[s] = rmax[s].max(req[s]);
            }
        }
    }
    let pct = |x: u32| if total > 0 { x as f32 * 100.0 / total as f32 } else { 0.0 };
    let mut out = format!("pv\t{k}\npvn\t{}\n", (1..=5).map(|n| format!("{:.1}", pct(n_by[n]))).collect::<Vec<_>>().join("\t"));
    for s in 0..5 {
        let (c0, r0) = if has[s] > 0 { (cmin[s], rmin[s]) } else { (0.0, 0) };
        out.push_str(&format!("pvs\t{s}\t{:.1}\t{:.0}\t{:.0}\t{r0}\t{}\n", pct(has[s]), c0, cmax[s], rmax[s]));
    }
    out
}

/// 防具本体原版的八种减伤比例（受伤倍率 v → 1 − v）：无属性、斩、打、突、魔、火、雷、圣（同 CUT_FIELDS 的顺序）
fn base_cuts(repo: &SoloParamRepository, base: u32) -> Option<[f32; 8]> {
    let p = repo.get::<EquipParamProtector>(base)?;
    Some([
        p.neutral_damage_cut_rate(),
        p.slash_damage_cut_rate(),
        p.blow_damage_cut_rate(),
        p.thrust_damage_cut_rate(),
        p.magic_damage_cut_rate(),
        p.fire_damage_cut_rate(),
        p.thunder_damage_cut_rate(),
        p.dark_damage_cut_rate(),
    ]
    .map(|v| 1.0 - v))
}

/// 一件防具在游戏参数里现在的行：八种减伤的平均（%）、重量（继承页按减伤率 / 重量排序，用户 2026-11-10）
fn armor_stats(repo: &SoloParamRepository, pid: u32) -> Option<(f32, f32)> {
    let p = repo.get::<EquipParamProtector>(pid)?;
    let v = [
        p.neutral_damage_cut_rate(),
        p.slash_damage_cut_rate(),
        p.blow_damage_cut_rate(),
        p.thrust_damage_cut_rate(),
        p.magic_damage_cut_rate(),
        p.fire_damage_cut_rate(),
        p.thunder_damage_cut_rate(),
        p.dark_damage_cut_rate(),
    ];
    Some((v.iter().map(|x| (1.0 - x) * 100.0).sum::<f32>() / 8.0, p.weight()))
}

fn esc(s: &str) -> String {
    s.replace('\t', " ").replace('\r', "").replace('\n', "\\n")
}

/// 快照（格式见文件头）
/// 图标：游戏参数里这一行现在的（容器行改写时、护符模板载入后都换成了带稀有度边框的）。cat 0 武器 / 1 防具（按人物性别，gender 0 = 女）/
/// 2 护符 / 别的 = 道具
fn param_icon(d: &Defs, repo: Option<&mut SoloParamRepository>, female: bool, cat: usize, id: u32) -> u16 {
    let Some(repo) = repo else { return 0 };
    let row = match cat {
        0 => param_mut::<EquipParamWeapon>(repo, id),
        1 => param_mut::<EquipParamProtector>(repo, id),
        2 => param_mut::<EquipParamAccessory>(repo, id),
        _ => return repo.get_mut::<EquipParamGoods>(id).map_or(0, |g| g.icon_id()),
    };
    let field = ICON_FIELDS[cat][if cat == 1 && female { 1 } else { 0 }];
    row.and_then(|r| get_u16(&d.lib, CAT_CODE[cat], r, field)).unwrap_or(0)
}

fn snapshot(d: &Defs, st: &St, pgd: &PlayerGameData, mode: u32) -> (String, Vec<IhArmor>, Vec<WRow>) {
    let lang = ui_lang();
    let goods = d.items.inherit.map_or(0, |i| i.goods);
    let mut repo = unsafe { SoloParamRepository::instance_mut() }.ok();
    let female = pgd.gender == 0;
    let icon = |repo: &mut Option<&mut SoloParamRepository>, cat: usize, id: u32| param_icon(d, repo.as_deref_mut(), female, cat, id);
    let mut out = format!(
        "v\t3\nlang\t{lang}\nstones\t{}\nstone_icon\t{}\n",
        if goods > 0 { count_goods(pgd, goods) } else { 0 },
        if goods > 0 { icon(&mut repo, 9, goods) } else { 0 }
    );
    if let Some(rf) = d.items.reforge {
        out.push_str(&format!("rstones\t{}\nrstone_icon\t{}\n", count_goods(pgd, rf.goods), icon(&mut repo, 9, rf.goods)));
    }
    if let Some(o) = d.items.overlimit {
        out.push_str(&format!(
            "ostones\t{}\t{}\nostone_icon\t{}\t{}\n",
            count_goods(pgd, o.gw),
            count_goods(pgd, o.ga),
            icon(&mut repo, 9, o.gw),
            icon(&mut repo, 9, o.ga)
        ));
    }
    // 七咒页
    if let Some(list) = crate::curses::ui_state(lang).filter(|_| mode == 2) {
        let ci = |n: &str| d.lib.cicon.get(n).copied().unwrap_or(0);
        out.push_str(&format!("cshard\t{}\t{}\t{}\n", ci("curse_shard"), ci("bless_shard"), ci("shard_empty")));
        for (k, (key, n, need, rev, region, t)) in list.iter().enumerate() {
            let ic = ci(&format!("{}_{key}", if *rev { "bless" } else { "curse" }));
            out.push_str(&format!("cur	{k}	{n}	{need}	{}	{}	{}	{}	{}	{}	{ic}
", *rev as u8, esc(&t[0]), esc(region), esc(&t[1]), esc(&t[2]), esc(&t[3])));
        }
    }
    // 魇印页
    if let Some(u) = crate::nightmare::ui_state().filter(|_| mode == 1) {
        // 这条词条的护符模板（这个稀有度没有就往下找）的图标
        let tal_icon = |repo: &mut Option<&mut SoloParamRepository>, key: &str, r: usize| -> u16 {
            let Some(f) = d.lib.fam_by_key.get(key).map(|&i| &d.lib.fams[i]) else { return 0 };
            (0..=r.min(3)).rev().chain(r.min(3) + 1..4).find_map(|x| f.r[x].as_ref().filter(|y| y.tal > 0)).map_or(0, |y| icon(repo, 2, y.tal as u32))
        };
        let csv = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        out.push_str(&format!("nmo\t{}\t{}\t{}\n", u.open, csv(&u.levels), csv(&u.need)));
        for (k, (key, color, cnt, lv)) in u.fams.iter().enumerate() {
            let next = u.levels.get(*lv).copied().unwrap_or(0);
            let now_t = if *lv > 0 { sentence(&d.lib, key, lv - 1, lang) } else { String::new() };
            let next_t = if *lv < 4 { sentence(&d.lib, key, *lv, lang) } else { String::new() };
            let ic = tal_icon(&mut repo, key, lv.saturating_sub(1));
            out.push_str(&format!(
                "nmf\t{k}\t{color}\t{cnt}\t{lv}\t{next}\t{}\t{}\t{}\t{ic}\n",
                esc(&suffix(&d.lib, key, lang)),
                esc(&strip_font(&now_t)),
                esc(&strip_font(&next_t))
            ));
        }
        for (i, s) in u.slots.iter().enumerate() {
            out.push_str(&format!("nms\t{i}\t{}\n", s.map_or(-1, |k| k as i64)));
        }
        for (k, r) in &u.embers {
            let ic = u.fams.get(*k).map_or(0, |f| tal_icon(&mut repo, &f.0, *r));
            out.push_str(&format!("nme\t{k}\t{r}\t{ic}\n"));
        }
    }
    // 装备工坊：防具、武器只给索引，明细界面按需要来（details）
    let (mut armor, mut weapons) = (Vec::new(), Vec::new());
    if mode == 0 {
        let stats = [pgd.strength, pgd.dexterity, pgd.intelligence, pgd.faith, pgd.arcane];
        out.push_str(&format!("pst\t{}\n", stats.map(|s| s.to_string()).join("\t")));
        weapons = weapon_index(d, st, pgd, &repo);
        for r in &weapons {
            out.push_str(&format!("w\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.k, r.rar, r.worn as u8, r.locked as u8, r.wtype, r.ovl as u8, r.sort, r.spec, r.discard as u8));
        }
        armor = ih_armor(d, st, pgd);
        armor.retain(|a| st.recs.contains_key(&(1, a.idx)));
        for a in &armor {
            let rec = &st.recs[&(1, a.idx)];
            let ovl = d.items.overlimit.is_some() && overlimit::armor_now(&d.lib, rec).is_some() && d.lib.ovlp[a.rar].is_some();
            let keys = match &rec.recipe {
                Recipe::P { sps, .. } if !sps.is_empty() => sps.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(","),
                _ => "-".to_string(),
            };
            let (cut, wt) = repo.as_deref().and_then(|r| armor_stats(r, a.pid)).unwrap_or((0.0, 0.0));
            out.push_str(&format!(
                "a\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{keys}\t{cut:.2}\t{wt:.1}\n",
                a.idx,
                a.rar,
                a.part,
                a.worn as u8,
                a.locked as u8,
                ovl as u8,
                a.sort
            ));
        }
    }
    (out, armor, weapons)
}

/// 界面要的（dr_ws_want 的一行）
#[derive(Debug, PartialEq)]
enum Want {
    A(Vec<u32>),
    W(Vec<u32>),
    O(u32, usize, Vec<u32>),
    S,
}

fn parse_want(text: &str) -> Vec<Want> {
    let ids = |s: &str| s.split(',').filter_map(|x| x.parse().ok()).collect::<Vec<u32>>();
    text.lines()
        .filter_map(|l| match l.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["a", v] => Some(Want::A(ids(v))),
            ["w", v] => Some(Want::W(ids(v))),
            ["o", s, k, v] => Some(Want::O(s.parse().ok()?, k.parse().ok()?, ids(v))),
            ["s"] => Some(Want::S),
            _ => None,
        })
        .collect()
}

/// 一件防具的明细（格式见文件头）：名字、图标、超限、词条；能当来源的还有提取成护符
fn armor_lines(d: &Defs, st: &St, repo: &mut Option<&mut SoloParamRepository>, female: bool, lang: usize, a: &IhArmor, out: &mut String) {
    let Some(rec) = st.recs.get(&(1, a.idx)) else { return };
    let name = esc(&plain_name(d, rec, lang));
    let ic = param_icon(d, repo.as_deref_mut(), female, 1, a.pid);
    out.push_str(&format!("tgt\t{}\t{}\t{}\t{}\t{}\t{ic}\t{name}\n", a.idx, a.rar, a.part, a.worn as u8, a.locked as u8));
    // 超限：现在的倍率和上限、本体原版的八种减伤（界面按倍率算之后的样子）
    if let (Some(o), Some(m), Some((med, hi, cap))) = (d.items.overlimit, overlimit::armor_now(&d.lib, rec), d.lib.ovlp[a.rar]) {
        // 超过原上限才算（升词条也会把倍率定住存进记录，只看存没存分不出来）
        let ovl = m > hi + 1e-6;
        let s = (cap - med) / o.steps.max(1) as f64;
        let need = overlimit::need(&d.lib, rec, &o);
        out.push_str(&format!("aovl\t{}\t{m:.4}\t{med:.4}\t{hi:.4}\t{cap:.4}\t{s:.5}\t{need}\t{}\n", a.idx, ovl as u8));
        if let Some(c) = repo.as_deref().and_then(|r| base_cuts(r, base_of(rec))) {
            out.push_str(&format!("acut\t{}\t{}\n", a.idx, c.map(|x| format!("{x:.4}")).join("\t")));
        }
    }
    if let Recipe::P { sps, pos, .. } = &rec.recipe {
        for (i, (sp, key)) in sps.iter().enumerate() {
            let r = sp_rar(&d.lib, key, *sp).unwrap_or(rec.rar());
            out.push_str(&format!(
                "aff\t{}\t{i}\t{}\t{r}\t{}\t{}\n",
                a.idx,
                fam_mech(&d.lib, key) as u8,
                esc(&suffix(&d.lib, key, lang)),
                esc(&sentence_at(&d.lib, key, r, pos_at(pos, i), lang))
            ));
            // 超限页的词条升级：升到哪一档、几颗、升级后的说明（不能升的写原因）
            if d.items.overlimit.is_some() {
                match overlimit::affix_up(&d.lib, rec, i) {
                    Ok((_, _, t)) => out.push_str(&format!(
                        "aup\t{}\t{i}\t1\t{t}\t{}\t{}\n",
                        a.idx,
                        overlimit::affix_cost(t),
                        esc(&sentence_at(&d.lib, key, t, pos_at(pos, i), lang))
                    )),
                    Err(w) => out.push_str(&format!("aup\t{}\t{i}\t0\t{r}\t0\t{}\n", a.idx, esc(w[lang]))),
                }
            }
        }
    }
    // 提取成护符（能当来源的才有）
    if a.worn || a.locked {
        return;
    }
    let Recipe::P { sps, pos, .. } = &rec.recipe else { return };
    for (k, (sp, key)) in sps.iter().enumerate() {
        let r = sp_rar(&d.lib, key, *sp).unwrap_or(rec.rar());
        match ih_talisman(&d.lib, key, r) {
            Some(tal) => {
                let tr = Rec { cat: 2, idx: 0, tier: r as u8, recipe: Recipe::A { tal: tal as u32, sp: *sp, key: key.clone(), pos: pos_at(pos, k) } };
                let ic = param_icon(d, repo.as_deref_mut(), female, 2, tal as u32);
                out.push_str(&format!("tal\t{}\t{k}\t1\t{ic}\t{}\n", a.idx, esc(&plain_name(d, &tr, lang))));
            }
            None => out.push_str(&format!("tal\t{}\t{k}\t0\t0\t{}\n", a.idx, ["这条词条没有护符", "this affix has no talisman"][lang])),
        }
    }
}

/// 来源第 k 条放到这几件目标上的放法（opt / nopt）
fn option_lines(d: &Defs, st: &St, lang: usize, src_idx: u32, k: usize, tgts: &[u32], out: &mut String) {
    let Some(src) = st.recs.get(&(1, src_idx)) else { return };
    for &t in tgts.iter().filter(|&&t| t != src_idx) {
        let Some(tgt) = st.recs.get(&(1, t)) else { continue };
        match ih_options(&d.lib, src, k, tgt) {
            Ok(v) => {
                for (rep, why) in v {
                    out.push_str(&format!(
                        "opt\t{src_idx}\t{k}\t{t}\t{}\t{}\t{}\n",
                        rep.map_or(-1, |x| x as i64),
                        why.is_none() as u8,
                        esc(why.as_ref().map_or("", |w| &w[lang]))
                    ));
                }
            }
            Err(w) => out.push_str(&format!("nopt\t{src_idx}\t{k}\t{t}\t{}\n", esc(&w[lang]))),
        }
    }
}

/// 关键词搜索用的文字（从第 pos 件起算一批）：防具 名字 + 各条词条的名字和说明，武器 名字 + 简介（补正、战技、特效）+ 说明。返回文字和下一批从哪开始（算完是 None）
fn search_lines(d: &Defs, st: &St, armor: &[IhArmor], weapons: &[WRow], pos: usize) -> (String, Option<usize>) {
    let lang = ui_lang();
    let total = armor.len() + weapons.len();
    let end = (pos + SEARCH_PER_TICK).min(total);
    let mut out = String::new();
    for i in pos..end {
        if let Some(a) = armor.get(i) {
            let Some(rec) = st.recs.get(&(1, a.idx)) else { continue };
            let (mut name, mut desc) = (plain_name(d, rec, lang), String::new());
            if let Recipe::P { sps, pos, .. } = &rec.recipe {
                for (i, (sp, key)) in sps.iter().enumerate() {
                    let r = sp_rar(&d.lib, key, *sp).unwrap_or(rec.rar());
                    name += &format!(" {}", suffix(&d.lib, key, lang));
                    desc += &format!("{} ", strip_font(&sentence_at(&d.lib, key, r, pos_at(pos, i), lang)));
                }
            }
            out.push_str(&format!("sa\t{}\t{}\x01{}\n", a.idx, esc(&name), esc(&desc)));
        } else if let Some(r) = weapons.get(i - armor.len()) {
            let Some(rec) = st.recs.get(&(0, r.k)) else { continue };
            let [name, info, desc] = compose_cap(&d.lib, rec, lang, d.items.cap[1]).map(|x| strip_font(&x));
            out.push_str(&format!("sw\t{}\t{} {}\x01{}\n", r.k, esc(&name), esc(&info), esc(&desc)));
        }
    }
    if end >= total {
        out.push_str("sdone\n");
        (out, None)
    } else {
        (out, Some(end))
    }
}

/// 算界面要的明细（只算要的那几件，一次滚动几行）
fn details(d: &Defs, st: &St, pgd: &PlayerGameData, want: &str, armor: &[IhArmor], weapons: &[WRow]) -> String {
    let lang = ui_lang();
    let mut repo = unsafe { SoloParamRepository::instance_mut() }.ok();
    let female = pgd.gender == 0;
    let mut out = String::new();
    for w in parse_want(want) {
        match w {
            Want::A(ids) => {
                for a in ids.iter().filter_map(|i| armor.iter().find(|x| x.idx == *i)) {
                    armor_lines(d, st, &mut repo, female, lang, a, &mut out);
                }
            }
            Want::W(ids) => {
                let rows: Vec<&WRow> = ids.iter().filter_map(|i| weapons.iter().find(|x| x.k == *i)).collect();
                weapon_lines(d, st, pgd, &mut repo, lang, &rows, &mut out);
            }
            Want::O(src, k, tgts) => option_lines(d, st, lang, src, k, &tgts, &mut out),
            Want::S => with(|w| w.search_pos = Some(0)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn want_text_parses() {
        let v = parse_want("a 1,2\nw 7\no 3 1 4,5\nnonsense\no x 1 4\ns\n");
        assert_eq!(v, vec![Want::A(vec![1, 2]), Want::W(vec![7]), Want::O(3, 1, vec![4, 5]), Want::S]);
    }
}
