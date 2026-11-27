//! 装备工坊「超限」页（用户 2026-11-03，超限锻造石；规则在 dark_core 的 gear\overlimit.rs，照「重铸」页的布局）：
//!   左：页签 武器 / 防具（X / Tab），背包里的生成武器 / 防具；超限过的标金色「已超限」，到顶的标「已到上限」
//!   中：武器照「重铸」页的原版详情；防具是名字、部位和八种减伤（现在 → 用了以后）
//!   右：每项补正（防具是减伤倍率）一条进度条：中位、原上限两条刻度，超过原上限那段金色，选中的按钮用了以后的部分半透明金色；
//!       三个按钮 用 1 颗 / 用 10 颗 / 用到上限，下面是消耗和身上有几颗
//!   防具页右边按钮下面还有「词条升级」（用户 2026-11-04「超限锻造石可以提升词条等级 最高还是传说」「只有防具石升防具」）：
//!       每条词条一行（名字、升到哪个稀有度、几颗防具石：升到稀有 1 颗、史诗 2 颗、传说 3 颗），选中时中间的词条区显示升级后的说明
//! 操作：上下选装备，A 到右边按钮、上下选按钮（防具接着往下是各条词条）、再 A 弹确认；Y 直接确认（当前按钮）；B 返回 / 关窗口；X 换武器 / 防具。
use super::reforge::{STAT, wtype_name};
use super::*;
use crate::data::{Aff, Aovl, Aup};

/// 超过原上限的金色（比标题的金色亮）
const OGOLD: [u8; 3] = [240, 200, 80];
const ONAME: [[&str; 2]; 2] = [["超限锻造石（武器）", "Overlimit Smithing Stone (Armament)"], ["超限锻造石（防具）", "Overlimit Smithing Stone (Armor)"]];
const OTABS: [&str; 2] = ["MENU_Tab_Weapon", "MENU_Tab_Armor"];
const OFILTER: [[&str; 2]; 2] = [["武器", "防具"], ["Armaments", "Armor"]];
/// 八种减伤（dark_core 快照 acut 的顺序：无属性 斩 打 突 魔 火 雷 圣）
const CUT: [[&str; 8]; 2] = [
    ["物理", "斩击", "打击", "突刺", "魔力", "火", "雷", "圣"],
    ["Physical", "Slash", "Strike", "Pierce", "Magic", "Fire", "Lightning", "Holy"],
];

/// 左边一行：生成武器 / 防具（和它的超限数据）
#[derive(Clone)]
pub(super) enum OItem {
    W(Wpn),
    A(Piece, Aovl),
}

impl OItem {
    fn need(&self) -> u32 {
        match self {
            OItem::W(w) => w.need,
            OItem::A(_, a) => a.need,
        }
    }

    fn ovl(&self) -> bool {
        match self {
            OItem::W(w) => w.ovl,
            OItem::A(_, a) => a.ovl,
        }
    }

    fn loaded(&self) -> bool {
        match self {
            OItem::W(w) => w.loaded,
            OItem::A(p, _) => p.loaded,
        }
    }

    fn icon(&self) -> u16 {
        match self {
            OItem::W(w) => w.icon,
            OItem::A(p, _) => p.icon,
        }
    }
}

impl Workshop {
    /// 左边列表（武器 / 防具页签，再按关键词、词条筛选）：装备中的排最前面（用户 2026-11-04「超限页也改」，同重铸页），然后按获得顺序（data.rs 的 ovl_view）
    pub(super) fn oview(&self) -> &[usize] {
        &self.v_ovl
    }

    fn oitem_at(&self, i: usize) -> Option<OItem> {
        let s = self.snap.as_ref()?;
        let &pos = self.oview().get(i)?;
        Some(if self.otab == 0 {
            OItem::W(s.weapons.get(pos)?.clone())
        } else {
            let p = s.pieces.get(pos)?;
            OItem::A(p.clone(), s.aovl.get(&p.idx).cloned().unwrap_or_default())
        })
    }

    fn oitem(&self) -> Option<OItem> {
        self.oitem_at(self.osel)
    }

    /// 身上这一页的石头
    fn ohave(&self) -> u32 {
        self.snap.as_ref().map_or(0, |s| s.ostones[self.otab.min(1)])
    }

    /// 选中的按钮实际用几颗：1 / 10 / 到上限，都不超过还要几颗和身上有几颗（词条升级的按钮不算）
    fn ouse_of(&self, btn: usize, need: u32) -> u32 {
        if btn > 2 {
            return 0;
        }
        [1, 10, need][btn].min(need).min(self.ohave())
    }

    /// 防具页选中的这件的各条词条和能不能升级（武器页、没选中是空）
    fn oaffs(&self) -> Vec<(Aff, Aup)> {
        let (Some(s), Some(OItem::A(p, _))) = (&self.snap, self.oitem()) else { return Vec::new() };
        let Some(v) = s.affs.get(&p.idx) else { return Vec::new() };
        v.iter().enumerate().map(|(i, a)| (a.clone(), s.aup.get(&(p.idx, i)).cloned().unwrap_or_default())).collect()
    }

    /// 选中的是第几条词条的升级按钮（防具页按钮 3 起）
    pub(super) fn oaff_sel(&self) -> Option<usize> {
        (self.otab == 1 && self.obtn >= 3).then(|| self.obtn - 3)
    }

    fn can_affix(&self) -> bool {
        let Some(i) = self.oaff_sel() else { return false };
        self.oaffs().get(i).is_some_and(|(_, u)| u.ok && u.cost <= self.ohave())
    }

    /// 右边最后一个按钮的序号：武器 3（0~2 用石头，3 弃置）；防具 2 + 词条数（防具没有弃置）
    fn obtn_max(&self) -> usize {
        if self.otab == 1 {
            2 + self.oaffs().len()
        } else {
            3
        }
    }

    /// 选中的是「弃置」按钮（只有武器栏有）
    pub(super) fn odiscard_sel(&self) -> bool {
        self.otab == 0 && self.obtn == 3
    }

    /// 能不能弃置：选中了一把武器、明细加载了、没装备着、没锁定
    fn can_discard(&self) -> bool {
        matches!(self.oitem(), Some(OItem::W(w)) if w.loaded && !w.worn && !w.locked)
    }

    fn ouse(&self) -> u32 {
        self.oitem().map_or(0, |it| self.ouse_of(self.obtn, it.need()))
    }

    fn can_overlimit(&self) -> bool {
        self.ouse() > 0
    }

    pub(super) fn handle_overlimit(&mut self, a: Act) {
        let n = self.oview().len();
        self.osel = self.osel.min(n.saturating_sub(1));
        self.obtn = self.obtn.min(self.obtn_max());
        if a.x {
            self.otab = 1 - self.otab.min(1);
            self.osel = 0;
            self.otop = 0;
            self.ocol = 0;
            return;
        }
        if self.ocol == 0 {
            if a.up && self.osel > 0 {
                self.osel -= 1;
            }
            if a.down && self.osel + 1 < n {
                self.osel += 1;
            }
            if (a.a || a.right || a.rb) && n > 0 {
                self.ocol = 1;
            }
        } else {
            if a.up && self.obtn > 0 {
                self.obtn -= 1;
            }
            if a.down && self.obtn < self.obtn_max() {
                self.obtn += 1;
            }
            if a.a {
                if self.odiscard_sel() {
                    // 弃置是静默切换（用户 2026-11-26）：不弹确认框，直接提请求，dark_core 改完名单回一句结果
                    if self.can_discard() {
                        if let Some(OItem::W(w)) = self.oitem() {
                            self.reqs.push(Req::Discard(w.idx));
                            self.waiting = true;
                            self.msg = Some((true, self.t("处理中……", "Working…").to_string()));
                        }
                    }
                } else {
                    self.confirm = self.can_overlimit() || self.can_affix();
                }
            } else if a.left || a.lb {
                self.ocol = 0;
            }
        }
        if a.y && (self.can_overlimit() || self.can_affix()) {
            self.confirm = true;
        }
        if a.b {
            if self.ocol > 0 {
                self.ocol = 0;
            } else {
                self.reqs.push(Req::Close);
            }
        }
    }

    /// 鼠标：页签、左边一行（点已经选中的 = A）、按钮（点了直接弹确认）
    pub(super) fn click_overlimit(&mut self, h: Hit) {
        let a = Act { a: true, ..Act::default() };
        match h {
            Hit::OTab(i) => {
                self.otab = i.min(1);
                self.osel = 0;
                self.otop = 0;
                self.ocol = 0;
            }
            Hit::OItem(i) => {
                let again = self.ocol == 0 && self.osel == i;
                self.ocol = 0;
                self.osel = i;
                if again {
                    self.handle_overlimit(a);
                }
            }
            Hit::OBtn(i) => {
                self.ocol = 1;
                self.obtn = i.min(self.obtn_max());
                self.handle_overlimit(a);
            }
            _ => {}
        }
    }

    pub(super) fn overlimit_go(&mut self) {
        self.confirm = false;
        let Some(it) = self.oitem() else { return };
        if let Some(i) = self.oaff_sel() {
            let OItem::A(p, _) = &it else { return };
            if !self.can_affix() {
                return;
            }
            self.reqs.push(Req::Affix { k: p.idx, i });
            self.waiting = true;
            self.msg = Some((true, self.t("处理中……", "Working…").to_string()));
            return;
        }
        let n = self.ouse();
        if n == 0 {
            return;
        }
        let (cat, k) = match &it {
            OItem::W(w) => (0, w.idx),
            OItem::A(p, _) => (1, p.idx),
        };
        self.reqs.push(Req::Overlimit { cat, k, n });
        self.waiting = true;
        self.msg = Some((true, self.t("处理中……", "Working…").to_string()));
    }

    pub(super) fn draw_overlimit(&mut self, d: &mut Draw) {
        let n = self.oview().len();
        self.osel = self.osel.min(n.saturating_sub(1));
        self.draw_olist(d);
        match self.oitem().filter(OItem::loaded) {
            Some(OItem::W(w)) => self.draw_wdetail_of(d, &w),
            Some(OItem::A(p, a)) => self.draw_adetail(d, &p, &a),
            None => d.sprite("MENU_FL_MenuBase", 614.0, 142.0, 792.0, 830.0, 0.9),
        }
        self.draw_oforge(d);
    }

    // 左：武器 / 防具页签 + 列表
    fn draw_olist(&mut self, d: &mut Draw) {
        let (x, y, w) = (70.0, 150.0, 520.0);
        let l = self.lang();
        d.sprite("MENU_BaseMain_02", x - 20.0, y - 10.0, w + 60.0, 820.0, 0.55);
        for (i, name) in OTABS.iter().enumerate() {
            let tx = x + 14.0 + i as f32 * 70.0;
            if i == self.otab {
                d.sprite("MENU_FL_Tab_Cursor", tx - 12.0, y - 6.0, 84.0, 72.0, 1.0);
            }
            d.sprite_fit(name, tx, y, 60.0, 58.0, if i == self.otab { 1.0 } else { 0.4 });
            self.hit(tx - 6.0, y - 4.0, 70.0, 66.0, Hit::OTab(i));
        }
        d.glyph("KG_XboxSeries_R_L", x + 300.0, y + 12.0, 34.0);
        d.text_clip_mid(x + 340.0, y + 29.0, 180.0, 22.0, GOLD, 1.0, OFILTER[l][self.otab.min(1)]);
        d.sprite("MENU_Line_01", x, y + 70.0, w, 3.0, 0.8);
        let n = self.oview().len();
        let (ly, rh, vis) = (y + 82.0, 76.0, LIST_VIS);
        fit_top(&mut self.otop, self.osel, vis, n, self.follow);
        self.hit(x, ly, w, rh * vis as f32, Hit::Area(Scroll::OItem));
        if n == 0 {
            let t = if self.filtering() {
                self.t("没有符合条件的装备", "No gear matches")
            } else if self.otab == 0 {
                self.t("背包里没有生成武器（仓库里的不算）", "No generated armaments in your inventory (storage doesn't count)")
            } else {
                self.t("背包里没有生成防具（仓库里的不算）", "No generated armor in your inventory (storage doesn't count)")
            };
            d.wrapped(x + 20.0, ly + 20.0, w - 40.0, 22.0, DIM, 1.0, t);
        }
        for i in window(self.otop, vis, n, 0) {
            let Some(it) = self.oitem_at(i) else { continue };
            let ry = ly + (i - self.otop) as f32 * rh;
            if i == self.osel {
                d.sprite("MENU_FL_Cursor_Wide", x - 8.0, ry + 2.0, w + 16.0, rh - 4.0, if self.ocol == 0 && !self.head { 1.0 } else { 0.45 });
            }
            if it.loaded() {
                let (icon, worn, locked, disc, rar, name, sub) = match &it {
                    OItem::W(wp) => (wp.icon, wp.worn, wp.locked, wp.discard, wp.rar, &wp.name, format!("{} · +{}", wtype_name(wp.wtype, l), wp.level)),
                    OItem::A(p, _) => (p.icon, p.worn, p.locked, false, p.rar, &p.name, PART[l][p.part.min(3)].to_string()),
                };
                d.icon(icon, x + 12.0, ry + 6.0, 64.0, 1.0);
                d.badges(x + 13.0, ry + 7.0, 20.0, 64.0, worn, locked, disc);
                d.text_clip(x + 92.0, ry + 10.0, w - 100.0, 24.0, RAR[rar.min(3)], 1.0, name);
                d.text(x + 92.0, ry + 44.0, 19.0, DIM, 1.0, &sub);
                let tag = if it.need() == 0 {
                    self.t("已到上限", "Maxed")
                } else if it.ovl() {
                    self.t("已超限", "Overlimited")
                } else {
                    ""
                };
                if !tag.is_empty() {
                    let tw = d.width(tag, 19.0);
                    d.text(x + w - 14.0 - tw, ry + 44.0, 19.0, OGOLD, 1.0, tag);
                }
            }
            self.hit(x, ry, w, rh, Hit::OItem(i));
        }
        for i in window(self.otop, vis, n, LIST_MARGIN).filter(|&i| i < self.otop || i >= self.otop + vis) {
            if let Some(it) = self.oitem_at(i) {
                d.preload(it.icon());
            }
        }
        self.bar(d, Scroll::OItem, x + w + 8.0, ly, rh * vis as f32, self.otop, vis, n);
        self.draw_kf_strip(d, x, ly + rh * vis as f32 + 6.0, w);
    }

    // 中（防具）：名字、部位，八种减伤 现在 → 用了以后
    fn draw_adetail(&mut self, d: &mut Draw, p: &Piece, a: &Aovl) {
        let (x, y, w) = (630.0, 150.0, 760.0);
        d.sprite("MENU_FL_MenuBase", x - 16.0, y - 8.0, w + 32.0, 830.0, 0.9);
        let l = self.lang();
        let banner = ["MENU_FL_Rar_d0", "MENU_FL_Rar_d1", "MENU_FL_Rar_d1", "MENU_FL_Rar_d2"][p.rar.min(3)];
        d.sprite(banner, x - 10.0, y, w + 20.0, 76.0, 0.9);
        d.text_clip(x + 20.0, y + 10.0, w - 40.0, 26.0, RAR[p.rar.min(3)], 1.0, &p.name);
        d.text_clip(x + 20.0, y + 44.0, w - 40.0, 19.0, DIM, 1.0, &format!("{}  ·  {}", PART[l][p.part.min(3)], RAR_NAME[l][p.rar.min(3)]));
        let used = self.ouse_of(self.obtn, a.need);
        let after = (a.m + a.step * used as f32).min(a.cap).max(a.m);
        let (ax, cw) = (x + 16.0, w - 32.0);
        let mut ay = y + 96.0;
        self.section(d, ax, ay, cw, self.t("减伤", "Damage Negation"));
        let cols = [ax + cw - 250.0, ax + cw - 110.0];
        d.text(cols[0], ay + 4.0, 18.0, DIM, 1.0, self.t("现在", "Now"));
        if used > 0 {
            d.text(cols[1], ay + 4.0, 18.0, DIM, 1.0, self.t("用了以后", "After"));
        }
        ay += 40.0;
        let neg = |m: f32, c: f32| (c * m).min(0.9) * 100.0;
        for i in 0..8 {
            d.text(ax + 12.0, ay, 21.0, TEXT, 0.9, CUT[l][i]);
            d.text(cols[0], ay, 21.0, if a.m > a.hi + 1e-4 { OGOLD } else { TEXT }, 1.0, &format!("{:.1}", neg(a.m, a.cut[i])));
            if used > 0 {
                d.text(cols[1], ay, 21.0, if after > a.hi + 1e-4 { OGOLD } else { TEXT }, 1.0, &format!("{:.1}", neg(after, a.cut[i])));
            }
            ay += 29.0;
        }
        ay += 12.0;
        self.section(d, ax, ay, cw, self.t("减伤倍率", "Negation Multiplier"));
        ay += 40.0;
        // 现在 ×1.740    原上限 ×1.800    超限上限 ×2.160（一行）
        let rows = [
            (self.t("现在", "Now"), format!("×{:.3}", a.m), if a.m > a.hi + 1e-4 { OGOLD } else { TEXT }),
            (self.t("原上限", "Original cap"), format!("×{:.3}", a.hi), DIM),
            (self.t("超限上限", "Overlimit cap"), format!("×{:.3}", a.cap), DIM),
        ];
        let colw = (cw - 24.0) / 3.0;
        for (j, (label, v, col)) in rows.into_iter().enumerate() {
            let cx = ax + 12.0 + colw * j as f32;
            let lw = d.width(label, 20.0);
            d.text(cx, ay, 20.0, TEXT, 0.9, label);
            d.text(cx + lw + 10.0, ay, 20.0, col, 1.0, &v);
        }
        ay += 30.0;
        ay += 6.0 + d.wrapped(ax + 12.0, ay + 10.0, cw - 24.0, 18.0, DIM, 1.0, self.t(
            "倍率乘在本体的减伤上，每种最多减 90%。超限会留在这件防具上，继承词条不会带走。",
            "The multiplier scales the base armor's negation, up to 90% each. Overlimit stays with this piece; inheriting affixes doesn't move it.",
        ));
        // 词条：各条的说明（按这条的稀有度上色）；选中它的升级按钮时下面接升级后的说明
        let affs = self.oaffs();
        if affs.is_empty() {
            return;
        }
        ay += 12.0;
        self.section(d, ax, ay, cw, self.t("词条", "Affixes"));
        ay += 40.0;
        let sel = self.oaff_sel();
        for (i, (a, u)) in affs.iter().enumerate() {
            if ay > 950.0 {
                break;
            }
            let col = RAR[a.rar.min(3)];
            let nw = d.width(&a.name, 20.0).min(cw * 0.45);
            d.text_clip(ax + 12.0, ay, cw * 0.45, 20.0, col, 1.0, &a.name);
            d.text(ax + 24.0 + nw, ay + 2.0, 17.0, DIM, 1.0, RAR_NAME[l][a.rar.min(3)]);
            ay += 26.0;
            ay += d.wrapped(ax + 24.0, ay, cw - 36.0, 17.0, TEXT, 0.9, &a.text) + 2.0;
            if sel == Some(i) && u.ok {
                let head = if l == 0 { format!("升到{}以后：", RAR_NAME[l][u.rar.min(3)]) } else { format!("As {}: ", RAR_NAME[l][u.rar.min(3)]) };
                ay += d.wrapped(ax + 24.0, ay, cw - 36.0, 17.0, RAR[u.rar.min(3)], 1.0, &format!("{head}{}", u.text)) + 2.0;
            }
            ay += 6.0;
        }
    }

    // 右：进度条、按钮、消耗
    fn draw_oforge(&mut self, d: &mut Draw) {
        let (x, y, w) = (1430.0, 150.0, 420.0);
        let l = self.lang();
        d.text(x, y + 6.0, 26.0, GOLD, 1.0, self.t("超限", "Overlimit"));
        d.sprite("MENU_Line_01", x, y + 46.0, w, 3.0, 0.8);
        let Some(it) = self.oitem() else { return };
        let bars: Vec<(String, [f32; 5])> = match &it {
            OItem::W(wp) => (0..5).filter_map(|k| wp.ocap[k].map(|c| (STAT[l][k].to_string(), c))).collect(),
            OItem::A(_, a) => vec![(self.t("减伤倍率", "Negation multiplier").to_string(), [a.m, a.med, a.hi, a.cap, a.step])],
        };
        let armor = matches!(it, OItem::A(..));
        let pct = bars.first().map_or(0.0, |(_, c)| if c[2] > 0.0 { (c[3] / c[2] - 1.0) * 100.0 } else { 0.0 });
        // 法杖、圣印记以外的武器每把的超限上限各有高低（dark_core overlimit::cap_rec）；2026-11-22 起不低于原上限，
        // 抽得最低的正好等于原上限（「再高 0%」不好看，单说「到原上限」）；老的 dark_core 还可能比原上限低
        let low = pct.round() < 0.0;
        let even = !armor && pct.round() == 0.0;
        let (lp, lz) = (-pct, pct.abs());
        let desc = match (armor, l) {
            (false, 0) if even => "每颗让所有有补正的能力各提高一点，最多到原上限（这把的超限上限）。超限过的武器不能再重铸。".to_string(),
            (false, _) if even => "Each stone raises every attribute scaling by one step, up to the original cap (this armament's overlimit cap). Overlimited armaments can't be reforged.".to_string(),
            (false, 0) if low => format!("每颗让所有有补正的能力各提高一点，最多到这把的超限上限（比原上限低 {lz:.0}%）。超限过的武器不能再重铸。"),
            (false, _) if low => format!("Each stone raises every attribute scaling by one step, up to this armament's overlimit cap ({lp:.0}% below the original cap). Overlimited armaments can't be reforged."),
            (false, 0) => format!("每颗让所有有补正的能力各提高一点，最多到原上限再高 {pct:.0}%（这把的超限上限）。超限过的武器不能再重铸。"),
            (false, _) => format!("Each stone raises every attribute scaling by one step, up to {pct:.0}% above the original cap (this armament's overlimit cap). Overlimited armaments can't be reforged."),
            (true, 0) => format!("每颗让减伤倍率（八种减伤一起）提高一点，最多到原上限再高 {pct:.0}%。"),
            (true, _) => format!("Each stone raises the negation multiplier (all eight types) by one step, up to {pct:.0}% above the original cap."),
        };
        let mut by = y + 58.0 + d.wrapped(x, y + 58.0, w, 19.0, DIM, 1.0, &desc) + 14.0;
        let used = self.ouse_of(self.obtn, it.need());
        let fmt = |v: f32| if armor { format!("×{v:.2}") } else { format!("{v:.1}") }; // 一颗只加不到 1 点，武器带一位小数
        for (label, [cur, med, hi, cap, step]) in &bars {
            let after = (cur + step * used as f32).min(*cap).max(*cur);
            d.text(x, by, 20.0, TEXT, 1.0, label);
            let val = if after > *cur { format!("{} → {}  / {}", fmt(*cur), fmt(after), fmt(*cap)) } else { format!("{}  / {}", fmt(*cur), fmt(*cap)) };
            let vw = d.width(&val, 20.0);
            d.text(x + w - vw, by, 20.0, if after.max(*cur) > hi + 1e-4 { OGOLD } else { TEXT }, 1.0, &val);
            // 条：最左是中位和现在里小的那个再往左一点，最右是超限上限；刻度 = 中位（暗）、原上限（金）
            let (bx, bw, bh, byy) = (x, w, 10.0, by + 32.0);
            let lo = cur.min(*med) - (cap - med) * 0.15;
            let px = |v: f32| bx + bw * ((v - lo) / (cap - lo).max(1e-6)).clamp(0.0, 1.0);
            d.rect(bx, byy, bw, bh, [40, 38, 34], 0.9);
            d.rect(bx, byy, px(cur.min(*hi)) - bx, bh, [150, 145, 130], 0.9);
            if cur > hi {
                d.rect(px(*hi), byy, px(*cur) - px(*hi), bh, OGOLD, 1.0);
            }
            if after > *cur {
                d.rect(px(*cur), byy, px(after) - px(*cur), bh, OGOLD, 0.45);
            }
            d.rect(px(*med) - 1.0, byy - 4.0, 2.0, bh + 8.0, DIM, 0.9);
            d.rect(px(*hi) - 1.5, byy - 6.0, 3.0, bh + 12.0, GOLD, 1.0);
            by += 60.0;
        }
        d.text(x, by - 6.0, 16.0, DIM, 1.0, self.t("刻度：暗 = 中位，金 = 原上限；金色部分超过原上限", "Ticks: dim = median, gold = original cap; gold fill is beyond the cap"));
        by += 30.0;
        // 按钮（武器栏 4 个：用 1 颗 / 用 10 颗 / 用到上限 / 弃置；防具栏只有前 3 个，接着是词条升级）
        let need = it.need();
        let have = self.ohave();
        let nb = if armor { 3 } else { 4 };
        let discarded = matches!(&it, OItem::W(wp) if wp.discard);
        for i in 0..nb {
            let (bx, bw, bh) = (x, w, 58.0);
            let focus = self.ocol == 1 && self.obtn == i;
            let n = self.ouse_of(i, need);
            if focus {
                d.sprite("MENU_FL_Cursor_Ent", bx, by, bw, bh, 1.0);
            }
            d.sprite("MENU_FL_Cursor_EntWaku", bx, by, bw, bh, if focus { 1.0 } else { 0.6 });
            let label = if i == 3 {
                self.t(if discarded { "取消弃置" } else { "弃置" }, if discarded { "Un-discard" } else { "Discard" }).to_string()
            } else {
                match (i, l) {
                    (0, 0) => "用 1 颗".to_string(),
                    (1, 0) => "用 10 颗".to_string(),
                    (_, 0) => format!("用到上限（还要 {need} 颗）"),
                    (0, _) => "Use 1".to_string(),
                    (1, _) => "Use 10".to_string(),
                    _ => format!("Use to cap ({need} needed)"),
                }
            };
            let lw = d.width(&label, 24.0);
            if i == self.obtn {
                d.glyph(if focus { "KG_XboxSeries_R_D" } else { "KG_XboxSeries_R_U" }, bx + (bw - lw) / 2.0 - 44.0, by + (bh - 34.0) / 2.0, 34.0);
            }
            let on = if i == 3 { self.can_discard() } else { n > 0 };
            d.text_mid(bx + (bw - lw) / 2.0, by + bh / 2.0, 24.0, if on { TEXT } else { DIM }, 1.0, &label);
            self.hit(bx, by, bw, bh, Hit::OBtn(i));
            by += bh + 10.0;
        }
        // 消耗
        let icon = self.snap.as_ref().map_or(0, |s| s.ostone_icon[self.otab.min(1)]);
        let cy = by + 6.0;
        d.icon(icon, x, cy, 40.0, 1.0);
        let name = ONAME[self.otab.min(1)][l];
        let aup = self.oaff_sel().and_then(|i| self.oaffs().get(i).map(|x| x.1.clone()));
        let (cost, col) = if let Some(u) = aup {
            if !u.ok {
                (u.text.clone(), DIM)
            } else if l == 0 {
                (format!("消耗 {name} ×{}（现有 {have}）", u.cost), if u.cost <= have { TEXT } else { ERRC })
            } else {
                (format!("Uses {} {name} (have {have})", u.cost), if u.cost <= have { TEXT } else { ERRC })
            }
        } else if need == 0 {
            (self.t("已经到超限上限", "Already at the overlimit cap").to_string(), DIM)
        } else if l == 0 {
            (format!("消耗 {name} ×{used}（现有 {have}）"), if used > 0 { TEXT } else { ERRC })
        } else {
            (format!("Uses {used} {name} (have {have})"), if used > 0 { TEXT } else { ERRC })
        };
        d.text_clip_mid(x + 48.0, cy + 20.0, w - 48.0, 19.0, col, 1.0, &cost);
        // 防具：词条升级（每条一行，按钮 3 + i）
        let affs = self.oaffs();
        if affs.is_empty() {
            return;
        }
        let mut ay = cy + 60.0;
        self.section(d, x, ay, w, self.t("词条升级", "Affix Upgrade"));
        ay += 42.0;
        ay += d.wrapped(x, ay, w, 17.0, DIM, 1.0, self.t(
            "让一条词条升一个稀有度，最高传说：升到稀有 1 颗、史诗 2 颗、传说 3 颗防具石。",
            "Raises one affix by one rarity, up to Legendary: 1 armor stone to Rare, 2 to Epic, 3 to Legendary.",
        )) + 10.0;
        for (i, (a, u)) in affs.iter().enumerate() {
            let (bh, b) = (54.0, 3 + i);
            let focus = self.ocol == 1 && self.obtn == b;
            if focus {
                d.sprite("MENU_FL_Cursor_Ent", x, ay, w, bh, 1.0);
            }
            d.sprite("MENU_FL_Cursor_EntWaku", x, ay, w, bh, if focus { 1.0 } else { 0.6 });
            if b == self.obtn {
                d.glyph(if focus { "KG_XboxSeries_R_D" } else { "KG_XboxSeries_R_U" }, x + 18.0, ay + (bh - 30.0) / 2.0, 30.0);
            }
            let ok = u.ok && u.cost <= have;
            let cy = ay + bh / 2.0;
            d.text_clip_mid(x + 56.0, cy, w * 0.4, 20.0, RAR[a.rar.min(3)], if u.ok { 1.0 } else { 0.55 }, &a.name);
            // 右边：→ 稀有度（上色）×几颗；不能升的写原因
            if u.ok {
                let rn = RAR_NAME[l][u.rar.min(3)];
                let tail = format!("  ×{}", u.cost);
                let (aw, rw, tw) = (d.width("→ ", 20.0), d.width(rn, 20.0), d.width(&tail, 20.0));
                let mut tx = x + w - 30.0 - aw - rw - tw;
                d.text_mid(tx, cy, 20.0, DIM, 1.0, "→ ");
                tx += aw;
                d.text_mid(tx, cy, 20.0, RAR[u.rar.min(3)], 1.0, rn);
                tx += rw;
                d.text_mid(tx, cy, 20.0, if ok { TEXT } else { ERRC }, 1.0, &tail);
            } else {
                let tw = d.width(&u.text, 18.0).min(w * 0.45);
                d.text_clip_mid(x + w - 30.0 - tw, cy, w * 0.45, 18.0, DIM, 1.0, &u.text);
            }
            self.hit(x, ay, w, bh, Hit::OBtn(b));
            ay += bh + 6.0;
        }
    }

    pub(super) fn draw_overlimit_confirm(&mut self, d: &mut Draw) {
        let Some(it) = self.oitem() else { return };
        let n = self.ouse();
        let l = self.lang();
        if let (Some(i), OItem::A(p, _)) = (self.oaff_sel(), &it) {
            let Some((a, u)) = self.oaffs().get(i).cloned() else { return };
            let stone = ONAME[1][l];
            let rn = RAR_NAME[l][u.rar.min(3)];
            let l1 = if l == 0 {
                format!("「{}」的「{}」升到{rn}：用 {} 颗{stone}", p.name, a.name, u.cost)
            } else {
                format!("Upgrade \"{}\" on \"{}\" to {rn}: use {} {stone}", a.name, p.name, u.cost)
            };
            let mut l2 = u.text.replace('\n', " ");
            if p.worn {
                l2.push_str(self.t("（穿着的防具换下再穿上才生效）", " (take it off and put it back on for it to apply)"));
            }
            self.confirm_box(d, &l1, &l2);
            return;
        }
        let (name, armor) = match &it {
            OItem::W(w) => (w.name.clone(), false),
            OItem::A(p, _) => (p.name.clone(), true),
        };
        let stone = ONAME[armor as usize][l];
        let l1 = if l == 0 { format!("超限「{name}」：用 {n} 颗{stone}") } else { format!("Overlimit \"{name}\": use {n} {stone}") };
        let l2 = if armor {
            self.t("超限会留在这件防具上，继承词条不会带走。", "Overlimit stays with this armor; inheriting affixes doesn't move it.")
        } else {
            self.t("超限过的武器不能再重铸。", "An overlimited armament can no longer be reforged.")
        };
        self.confirm_box(d, &l1, l2);
    }

    /// 确认框：两行字（第二行金色）、确定 / 取消
    fn confirm_box(&mut self, d: &mut Draw, l1: &str, l2: &str) {
        d.rect(-2000.0, -2000.0, 6000.0, 6000.0, [0, 0, 0], 0.5);
        d.band(-100.0, 400.0, 2120.0, 280.0, 0.92);
        d.sprite("MENU_FL_Dialog", -40.0, 410.0, 2000.0, 260.0, 0.8);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 418.0, 1600.0, 22.0, 0.9);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 640.0, 1600.0, 22.0, 0.9);
        for (i, t) in [l1, l2].iter().enumerate() {
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
