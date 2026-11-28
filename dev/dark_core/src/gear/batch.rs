//! 发货流水线 + 掉落池（用户 2026-11-04「顺便改一下生成的架构 怎么可拓展性这么差」）。
//!
//! 以前一键鉴定、定向鉴定、向星辰许愿各写一遍「挑类别 / 稀有度 → 生成 → 占容器 → 数发货表行数 → 写发货表 → 等发货 → 扣未鉴定道具」，
//! 产出只能是生成装备：要加一种产出（比如混进未鉴定传说护符里的传说疫病护符）得每条路都改。现在分三层：
//! - 来源（gear.rs 的 identify_batch / directed_batch / wish_batch）只列出要做的东西（`Want`：类、档、偏向、换掉的未鉴定道具）
//!   和每件的价钱（`run` 的 price，真要做这一件时才问），自己管请求、收钱、写日志。
//! - `run`：每件先查掉落池（`Pool`：items.txt 的 pool 行，重随写），没换成固定物品就按偏向生成装备；生成装备要占容器
//!   （这一类没空位就跳过，记进 full），固定物品不占；做到发货表行数或卢恩预算为止。
//! - `ship` / `deliver_check`（gear.rs）：两种产出（`Made`）一样写进发货表、等发货、到了扣未鉴定道具；生成装备另外记进角色记录。
//! 以后往鉴定 / 许愿里加东西：重随加一行 pool（rr/containers.py 写 ctx.unid_pool）；加新的来源：给 Src 加一项，列出 Want 调 run。

use super::*;

/// 发货的来源（pool 行里的来源名）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Src {
    Identify,
    Directed,
    Wish,
}

impl Src {
    fn key(self) -> &'static str {
        match self {
            Src::Identify => "identify",
            Src::Directed => "directed",
            Src::Wish => "wish",
        }
    }
}

/// 掉落池的一项：这些来源做这一类、这几档的东西时，有 pct% 换成这件固定物品（发货表的类别 lot、编号 id、个数 n）
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Pool {
    pub(super) srcs: Vec<String>,
    pub(super) cat: usize,
    pub(super) tiers: Vec<usize>,
    pub(super) pct: f64,
    pub(super) lot: i32,
    pub(super) id: i32,
    pub(super) n: u8,
    pub(super) name: String,
}

impl Pool {
    /// items.txt：pool <来源,…> <类 0~2> <档,…> <几率 %> <发货表类别> <编号> [个数] [名字]；看不懂返回 None（这一行不要，别的照用）
    pub(super) fn parse(f: &[&str]) -> Option<Pool> {
        let srcs: Vec<String> = f.get(1)?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let tiers = f.get(3)?.split(',').map(|x| x.trim().parse().ok()).collect::<Option<Vec<usize>>>()?;
        let p = Pool {
            srcs,
            cat: f.get(2)?.parse().ok()?,
            tiers,
            pct: f.get(4)?.parse().ok()?,
            lot: f.get(5)?.parse().ok()?,
            id: f.get(6)?.parse().ok()?,
            n: f.get(7).map_or(Some(1), |x| x.parse().ok())?,
            name: f.get(8).map(|s| s.to_string()).unwrap_or_default(),
        };
        (!p.srcs.is_empty() && p.cat < NCAT && !p.tiers.is_empty() && p.pct > 0.0 && p.id > 0 && p.n > 0).then_some(p)
    }

    fn hits(&self, src: Src, cat: usize, tier: usize) -> bool {
        self.cat == cat && self.tiers.contains(&tier) && self.srcs.iter().any(|s| s == src.key())
    }
}

/// 这一件换不换成掉落池里的固定物品：管得着的各项按顺序各掷一次（几率是对这一件的），中了取第一项
pub(super) fn pool_pick<'a>(pools: &'a [Pool], src: Src, cat: usize, tier: usize, rng: &mut Rng) -> Option<&'a Pool> {
    pools.iter().filter(|p| p.hits(src, cat, tier)).find(|p| (rng.below(1_000_000) as f64) < p.pct * 10_000.0)
}

/// 生成装备往哪边偏：不偏 / 按加点（武器）/ 指定武器类型并按加点 / 指定防具部位 / 指定护符流派（后两个是防具、护符鉴定，用户 2026-11-09）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Bias {
    None,
    Build([u32; 5]),
    Typed(u16, [u32; 5]),
    /// 防具部位：0 头 / 1 身 / 2 手 / 3 腿
    Slot(u8),
    /// 护符流派：Lib.arch_names 的序号
    Arch(u8),
}

/// 要做的一件：类、档（0~4）、偏向、换掉的未鉴定道具（到了扣 1 个；0 = 没有）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Want {
    pub(super) cat: usize,
    pub(super) tier: usize,
    pub(super) bias: Bias,
    pub(super) goods: u32,
}

/// 做出来的：生成装备（写进容器、记进角色记录）或固定物品（发货表一格，不占容器）；cat / tier 是 Want 的，算日志、价钱用
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Made {
    Gear(Rec),
    Item { cat: usize, tier: u8, lot: i32, id: i32, n: u8, name: String },
}

impl Made {
    pub(super) fn cat(&self) -> usize {
        match self {
            Made::Gear(r) => r.cat,
            Made::Item { cat, .. } => *cat,
        }
    }

    pub(super) fn tier(&self) -> usize {
        match self {
            Made::Gear(r) => r.tier as usize,
            Made::Item { tier, .. } => *tier as usize,
        }
    }

    pub(super) fn rec(&self) -> Option<&Rec> {
        match self {
            Made::Gear(r) => Some(r),
            Made::Item { .. } => None,
        }
    }
}

/// 一批做出来的：(产出, 换掉的未鉴定道具, 价钱)；full = 各类没容器跳过的件数；failed = 生成失败；
/// poor = 卢恩预算不够停下的；capped = 数到发货表行数停下的
#[derive(Debug, Default)]
pub(super) struct Batch {
    pub(super) made: Vec<(Made, u32, u32)>,
    pub(super) full: [u32; NCAT],
    pub(super) failed: u32,
    pub(super) poor: bool,
    pub(super) capped: bool,
}

impl Batch {
    /// 交给 ship 的：(产出, 换掉的未鉴定道具)
    pub(super) fn items(&self) -> Vec<(Made, u32)> {
        self.made.iter().map(|(m, g, _)| (m.clone(), *g)).collect()
    }

    pub(super) fn any_full(&self) -> bool {
        self.full.iter().any(|&x| x > 0)
    }
}

fn make_gear(lib: &Lib, w: &Want, rng: &mut Rng) -> Option<Recipe> {
    match w.bias {
        Bias::Typed(t, s) if w.cat == 0 => gen_weapon_for_typed(lib, w.tier, t, s, rng),
        Bias::Build(s) if w.cat == 0 => gen_weapon_for(lib, w.tier, Some(s), rng),
        Bias::Slot(x) if w.cat == 1 => gen_armor_slot(lib, w.tier, x, rng),
        Bias::Arch(a) if w.cat == 2 => gen_talisman_arch(lib, w.tier, a, rng),
        _ => generate(lib, w.cat, w.tier, rng),
    }
}

/// 做一批：wants 依次做，数到发货表行数为止；price 给每件定价（做得出、放得下时才问），花的加起来不超过 budget（超了就停）
pub(super) fn run(d: &Defs, st: &St, pgd: &PlayerGameData, src: Src, wants: impl IntoIterator<Item = Want>, budget: u32,
                  mut price: impl FnMut(&Want) -> u32) -> Batch {
    let held = held(d, pgd);
    // 弃置名单里卖掉 / 丢掉的武器条目先修剪掉，那些格子才能用（free 不用名单里的格子）
    if let Some(who) = st.who.as_ref() {
        let pruned = discard_prune(pgd, who);
        if pruned > 0 {
            log(&format!("弃置名单：清掉 {pruned} 条（背包和木箱里都没有了）"));
        }
    }
    let mut free = free(d, st, &held);
    let mut rng = Rng::new();
    let mut b = Batch::default();
    let mut spent = 0u64;
    for w in wants {
        if b.made.len() as u32 >= d.items.deliver_rows {
            b.capped = true;
            break;
        }
        let fixed = pool_pick(&d.items.pools, src, w.cat, w.tier, &mut rng);
        if fixed.is_none() && free[w.cat].is_empty() {
            b.full[w.cat] += 1;
            continue;
        }
        let made = match fixed {
            Some(p) => Made::Item { cat: w.cat, tier: w.tier as u8, lot: p.lot, id: p.id, n: p.n, name: p.name.clone() },
            None => match make_gear(&d.lib, &w, &mut rng) {
                Some(recipe) => Made::Gear(Rec { cat: w.cat, idx: free[w.cat][0], tier: w.tier as u8, recipe }),
                None => {
                    b.failed += 1;
                    continue;
                }
            },
        };
        let cost = price(&w);
        if spent + cost as u64 > budget as u64 {
            b.poor = true;
            break;
        }
        spent += cost as u64;
        if made.rec().is_some() {
            free[w.cat].remove(0);
        }
        b.made.push((made, w.goods, cost));
    }
    if b.failed > 0 {
        log(&format!("新装备系统（{}）：{} 件生成失败（词条库里没有能用的本体 / 词条）", src.key(), b.failed));
    }
    b
}

/// 日志用：「武器 3 / 防具 5 / 护符 1」，有固定物品的再加「；固定物品：疫病护符 ×1」
pub(super) fn made_text(ok: &[(Made, u32)]) -> String {
    let by_cat: Vec<String> = (0..NCAT).map(|c| format!("{} {}", CAT_ZH[c], ok.iter().filter(|x| x.0.cat() == c).count())).collect();
    let mut fixed: Vec<(String, u32)> = Vec::new();
    for (m, _) in ok {
        if let Made::Item { id, n, name, .. } = m {
            let k = if name.is_empty() { id.to_string() } else { name.clone() };
            match fixed.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1 += *n as u32,
                None => fixed.push((k, *n as u32)),
            }
        }
    }
    let mut s = by_cat.join(" / ");
    if !fixed.is_empty() {
        s += &format!("；其中固定物品：{}", fixed.iter().map(|(k, n)| format!("{k} ×{n}")).collect::<Vec<_>>().join("、"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pool 行：来源、类、档、几率、发货表类别、编号、个数、名字；缺的 / 看不懂的整行不要
    #[test]
    fn parse_pool() {
        let f: Vec<&str> = "pool\tidentify,wish\t2\t3,4\t3\t4\t4144\t1\t传说 疫病护符".split('\t').collect();
        let p = Pool::parse(&f).unwrap();
        assert_eq!((p.srcs.clone(), p.cat, p.tiers.clone(), p.pct, p.lot, p.id, p.n, p.name.as_str()),
                   (vec!["identify".to_string(), "wish".to_string()], 2, vec![3, 4], 3.0, 4, 4144, 1, "传说 疫病护符"));
        let short: Vec<&str> = "pool\tidentify\t2\t3\t3\t4\t4144".split('\t').collect();
        assert_eq!(Pool::parse(&short).map(|p| (p.n, p.name)), Some((1, String::new())));
        for bad in ["pool\tidentify\t2\t3\t0\t4\t4144", "pool\t\t2\t3\t3\t4\t4144", "pool\tidentify\t5\t3\t3\t4\t4144",
                    "pool\tidentify\t2\tx\t3\t4\t4144", "pool\tidentify\t2\t3\t3\t4"] {
            assert!(Pool::parse(&bad.split('\t').collect::<Vec<_>>()).is_none(), "{bad}");
        }
    }

    /// 掉落池：来源、类、档都对得上才掷；100% 必中、几率大致对
    #[test]
    fn pick_from_pool() {
        let p = |pct: f64| Pool { srcs: vec!["identify".into()], cat: 2, tiers: vec![3, 4], pct, lot: 4, id: 4144, n: 1, name: String::new() };
        let mut rng = Rng(7);
        let all = [p(100.0)];
        assert!(pool_pick(&all, Src::Identify, 2, 3, &mut rng).is_some());
        assert!(pool_pick(&all, Src::Identify, 2, 4, &mut rng).is_some());
        assert!(pool_pick(&all, Src::Wish, 2, 3, &mut rng).is_none());
        assert!(pool_pick(&all, Src::Identify, 1, 3, &mut rng).is_none());
        assert!(pool_pick(&all, Src::Identify, 2, 2, &mut rng).is_none());
        let some = [p(3.0)];
        let hit = (0..100_000).filter(|_| pool_pick(&some, Src::Identify, 2, 3, &mut rng).is_some()).count();
        assert!((2_500..3_500).contains(&hit), "{hit}");
    }

    /// items.txt 里的 pool 行：看得懂的收进 pools，看不懂的那一行丢掉、别的定义照用
    #[test]
    fn pools_in_items_txt() {
        let base = "container\tw\t240000000\t10000\t2\t10\t\ncontainer\tp\t30000000\t100\t2\t10\t\ncontainer\ta\t20000\t1\t2\t10\t\n\
                    text\tN\t128\ntext\tI\t160\ntext\tC\t640\nlabel\tzh\tx\nlabel\ten\ty\ngoods\t2\t3\t8023\n\
                    deliver\t10900\t32\t93501\nidentify\t93502\nlotlayout\t44\t0\t32\t40\t36\t28\n";
        let it = parse_items(&format!("{base}pool\tidentify\t2\t3,4\t3\t4\t4144\t1\t传说 疫病护符\npool\tidentify\t2\t3\t0\t4\t4144\n")).unwrap();
        assert_eq!(it.pools.iter().map(|p| (p.id, p.tiers.clone())).collect::<Vec<_>>(), vec![(4144, vec![3, 4])]);
        assert_eq!((it.deliver_rows, it.goods.get(&8023)), (32, Some(&(2, 3))));
        if let Some((items, _)) = super::super::tests::real_defs() {
            eprintln!("真实 items.txt 的掉落池：{:?}", items.pools);
        }
    }

    #[test]
    fn made_text_lists_fixed_items() {
        let ok = vec![(Made::Item { cat: 2, tier: 3, lot: 4, id: 4144, n: 1, name: "传说 疫病护符".into() }, 9003),
                      (Made::Item { cat: 2, tier: 3, lot: 4, id: 4144, n: 1, name: "传说 疫病护符".into() }, 9003)];
        assert_eq!(made_text(&ok), "武器 0 / 防具 0 / 护符 2；其中固定物品：传说 疫病护符 ×2");
    }
}
