//! 装备工坊「重铸」页（用户 2026-10-31：武器重铸集成进工坊；左边武器图标、中间武器详细信息都照原版，右边重铸按钮和重铸后补正、需求的范围）：
//!   左：背包里的生成武器（装备着的标原版「装备中」标记、锁定的标锁），上面一排类别页签（X / Tab）
//!   中：原版武器详情那样——名字（稀有度名牌底）、类型 · 强化 · 战技 · 重量，攻击力（基础 + 属性加成，dark_core 照原版算的）、
//!       防御时减伤、能力加成（补正数值）、必需能力值（不够的标红）、附加效果
//!   右：「重铸」按钮、「快速重铸」按钮、消耗（武器重铸石）、重铸后的范围（dark_core 按现在的加点抽很多次统计的：每项属性有补正的几率、补正和需求的范围、补正项数）
//! 操作：上下选武器，A 到右边按钮、再 A 弹确认；Y（键盘 R）快速重铸：不弹确认直接重铸，按一下重铸一次、按住不连发
//! （用户 2026-11-19「给重铸添加一个无需确认的快速重铸」）；B 返回 / 关窗口；X 换类别。
use super::*;

/// 武器类型名（原版 GR_MenuText 60010~60187 的文字，按 wepType）
const WTYPE: [(u16, &str, &str); 42] = [
    (1, "短剑", "Dagger"),
    (3, "直剑", "Straight Sword"),
    (5, "大剑", "Greatsword"),
    (7, "特大剑", "Colossal Sword"),
    (9, "曲剑", "Curved Sword"),
    (11, "大曲剑", "Curved Greatsword"),
    (13, "刀", "Katana"),
    (14, "双头剑", "Twinblade"),
    (15, "刺剑", "Thrusting Sword"),
    (16, "重刺剑", "Heavy Thrusting Sword"),
    (17, "斧", "Axe"),
    (19, "大斧", "Greataxe"),
    (21, "槌", "Hammer"),
    (23, "大槌", "Great Hammer"),
    (24, "连枷", "Flail"),
    (25, "矛", "Spear"),
    (28, "大矛", "Great Spear"),
    (29, "戟", "Halberd"),
    (31, "镰刀", "Reaper"),
    (35, "拳头", "Fist"),
    (37, "钩爪", "Claw"),
    (39, "软鞭", "Whip"),
    (41, "特大武器", "Colossal Weapon"),
    (50, "小弓", "Light Bow"),
    (51, "长弓", "Bow"),
    (53, "大弓", "Greatbow"),
    (55, "弩", "Crossbow"),
    (56, "弩炮", "Ballista"),
    (57, "手杖", "Glintstone Staff"),
    (61, "圣印记", "Sacred Seal"),
    (65, "小盾", "Small Shield"),
    (67, "中盾", "Medium Shield"),
    (69, "大盾", "Greatshield"),
    (87, "火把", "Torch"),
    (88, "格斗术", "Hand-to-Hand"),
    (89, "调香瓶", "Perfume Bottle"),
    (90, "突刺盾", "Thrusting Shield"),
    (91, "投掷剑", "Throwing Blade"),
    (92, "反手剑", "Backhand Blade"),
    (93, "轻大剑", "Light Greatsword"),
    (94, "大刀", "Great Katana"),
    (95, "兽爪", "Beast Claw"),
];
pub(super) const STAT: [[&str; 5]; 2] = [["力气", "灵巧", "智力", "信仰", "感应"], ["Str", "Dex", "Int", "Fai", "Arc"]];
const ELEM: [[&str; 5]; 2] = [["物理", "魔力", "火", "雷", "圣"], ["Physical", "Magic", "Fire", "Lightning", "Holy"]];
/// 类别页签：全部 / 近战 / 远程与法器 / 盾牌
const WTABS: [&str; 4] = ["MENU_Tab_Weapon", "MENU_Tab_07", "MENU_Tab_08", "MENU_Tab_09"];
const WFILTER: [[&str; 4]; 2] = [["全部", "近战", "远程 / 法器", "盾牌"], ["All", "Melee", "Ranged / Catalysts", "Shields"]];

pub(super) fn wtype_name(t: u16, l: usize) -> &'static str {
    WTYPE.iter().find(|x| x.0 == t).map_or("", |x| if l == 0 { x.1 } else { x.2 })
}

impl Workshop {
    /// 左边列表：按类别、关键词、词条筛选；装备中的排最前面（用户 2026-11-04「重铸页面左边自动把装备中的武器放在第一位」），然后按获得顺序（data.rs 的 wpn_view）
    pub(super) fn wview(&self) -> &[usize] {
        &self.v_wpn
    }

    fn weapon_at(&self, i: usize) -> Option<Wpn> {
        let s = self.snap.as_ref()?;
        s.weapons.get(*self.wview().get(i)?).cloned()
    }

    fn weapon(&self) -> Option<Wpn> {
        self.weapon_at(self.rsel)
    }

    fn stones_left(&self) -> u32 {
        self.snap.as_ref().map_or(0, |s| s.rstones)
    }

    pub(super) fn handle_reforge(&mut self, a: Act) {
        let n = self.wview().len();
        self.rsel = self.rsel.min(n.saturating_sub(1));
        if a.x {
            self.rfilter = (self.rfilter + 1) % 4;
            self.rsel = 0;
            self.rtop = 0;
            self.rcol = 0;
            return;
        }
        if self.rcol == 0 {
            if a.up && self.rsel > 0 {
                self.rsel -= 1;
            }
            if a.down && self.rsel + 1 < n {
                self.rsel += 1;
            }
            if (a.a || a.right || a.rb) && n > 0 {
                self.rcol = 1;
            }
        } else if a.a {
            self.confirm = self.weapon().is_some_and(|w| !w.ovl); // 超限过的不能重铸
        } else if a.left || a.lb {
            self.rcol = 0;
        }
        if a.y {
            self.quick_reforge();
        }
        if a.b {
            if self.rcol > 0 {
                self.rcol = 0;
            } else {
                self.reqs.push(Req::Close);
            }
        }
    }

    /// 能快速重铸：选中了武器、没超限过、还有重铸石（石头不够时 dark_core 也会拒）
    pub(super) fn can_quick_reforge(&self) -> bool {
        self.stones_left() > 0 && self.weapon().is_some_and(|w| !w.ovl)
    }

    /// 快速重铸：不弹确认，直接重铸选中的武器（等 dark_core 回了结果才能再来一次：waiting 时 handle 不收操作）
    pub(super) fn quick_reforge(&mut self) {
        match self.weapon() {
            None => {}
            Some(w) if w.ovl => self.msg = Some((false, self.t("这把武器超限过，不能再重铸", "This armament has been overlimited and can't be reforged").to_string())),
            Some(_) if self.stones_left() == 0 => self.msg = Some((false, self.t("没有武器重铸石", "You have no Weapon Reforge Stone").to_string())),
            Some(_) => self.reforge_go(),
        }
    }

    pub(super) fn reforge_go(&mut self) {
        self.confirm = false;
        let Some(w) = self.weapon() else { return };
        self.reqs.push(Req::Reforge(w.idx));
        self.waiting = true;
        self.msg = Some((true, self.t("处理中……", "Working…").to_string()));
    }

    pub(super) fn draw_reforge(&mut self, d: &mut Draw) {
        // 选中的武器换了就要它重铸后的范围（重铸完 dark_core 自己会重算）
        let cur = self.weapon().map(|w| w.idx);
        if cur != self.prev_for {
            self.prev_for = cur;
            self.preview = None;
            if let Some(k) = cur {
                self.reqs.push(Req::Preview(k));
            }
        }
        self.draw_wlist(d);
        self.draw_wdetail(d);
        self.draw_forge(d);
    }

    // 左：类别页签 + 武器列表
    fn draw_wlist(&mut self, d: &mut Draw) {
        let (x, y, w) = (70.0, 150.0, 520.0);
        let l = self.lang();
        d.sprite("MENU_BaseMain_02", x - 20.0, y - 10.0, w + 60.0, 820.0, 0.55);
        for (i, name) in WTABS.iter().enumerate() {
            let tx = x + 14.0 + i as f32 * 70.0;
            if i == self.rfilter {
                d.sprite("MENU_FL_Tab_Cursor", tx - 12.0, y - 6.0, 84.0, 72.0, 1.0);
            }
            d.sprite_fit(name, tx, y, 60.0, 58.0, if i == self.rfilter { 1.0 } else { 0.4 });
            self.hit(tx - 6.0, y - 4.0, 70.0, 66.0, Hit::WTab(i));
        }
        d.glyph("KG_XboxSeries_R_L", x + 300.0, y + 12.0, 34.0);
        d.text_clip_mid(x + 340.0, y + 29.0, 180.0, 22.0, GOLD, 1.0, WFILTER[l][self.rfilter]);
        d.sprite("MENU_Line_01", x, y + 70.0, w, 3.0, 0.8);
        let n = self.wview().len();
        let (ly, rh, vis) = (y + 82.0, 76.0, LIST_VIS);
        fit_top(&mut self.rtop, self.rsel, vis, n, self.follow);
        self.hit(x, ly, w, rh * vis as f32, Hit::Area(Scroll::Wpn));
        if n == 0 {
            let msg = if self.filtering() { self.t("没有符合条件的武器", "No armaments match") } else { self.t("背包里没有生成武器（仓库里的不算）", "No generated armaments in your inventory (storage doesn't count)") };
            d.wrapped(x + 20.0, ly + 20.0, w - 40.0, 22.0, DIM, 1.0, msg);
        }
        for i in window(self.rtop, vis, n, 0) {
            let Some(wp) = self.weapon_at(i) else { continue };
            let ry = ly + (i - self.rtop) as f32 * rh;
            if i == self.rsel {
                d.sprite("MENU_FL_Cursor_Wide", x - 8.0, ry + 2.0, w + 16.0, rh - 4.0, if self.rcol == 0 && !self.head { 1.0 } else { 0.45 });
            }
            if wp.loaded {
                d.icon(wp.icon, x + 12.0, ry + 6.0, 64.0, 1.0);
                d.badges(x + 13.0, ry + 7.0, 20.0, 64.0, wp.worn, wp.locked, wp.discard);
                d.text_clip(x + 92.0, ry + 10.0, w - 100.0, 24.0, RAR[wp.rar.min(3)], 1.0, &wp.name);
                let sub = format!("{} · +{}", wtype_name(wp.wtype, l), wp.level);
                d.text(x + 92.0, ry + 44.0, 19.0, DIM, 1.0, &sub);
                if wp.ovl {
                    let tag = self.t("已超限", "Overlimited");
                    let tw = d.width(tag, 19.0);
                    d.text(x + w - 14.0 - tw, ry + 44.0, 19.0, DIM, 1.0, tag);
                }
            }
            self.hit(x, ry, w, rh, Hit::Wpn(i));
        }
        for i in window(self.rtop, vis, n, LIST_MARGIN).filter(|&i| i < self.rtop || i >= self.rtop + vis) {
            if let Some(wp) = self.weapon_at(i) {
                d.preload(wp.icon);
            }
        }
        self.bar(d, Scroll::Wpn, x + w + 8.0, ly, rh * vis as f32, self.rtop, vis, n);
        self.draw_kf_strip(d, x, ly + rh * vis as f32 + 6.0, w);
    }

    // 中：原版武器详情那样
    fn draw_wdetail(&mut self, d: &mut Draw) {
        match self.weapon().filter(|w| w.loaded) {
            Some(wp) => self.draw_wdetail_of(d, &wp),
            None => d.sprite("MENU_FL_MenuBase", 614.0, 142.0, 792.0, 830.0, 0.9),
        }
    }

    /// 一把武器的详情（「超限」页也用）
    pub(super) fn draw_wdetail_of(&mut self, d: &mut Draw, wp: &Wpn) {
        let (x, y, w) = (630.0, 150.0, 760.0);
        d.sprite("MENU_FL_MenuBase", x - 16.0, y - 8.0, w + 32.0, 830.0, 0.9);
        let l = self.lang();
        let stats = self.snap.as_ref().map_or([0; 5], |s| s.stats);
        let banner = ["MENU_FL_Rar_d0", "MENU_FL_Rar_d1", "MENU_FL_Rar_d1", "MENU_FL_Rar_d2"][wp.rar.min(3)];
        d.sprite(banner, x - 10.0, y, w + 20.0, 76.0, 0.9);
        d.text_clip(x + 20.0, y + 10.0, w - 40.0, 26.0, RAR[wp.rar.min(3)], 1.0, &wp.name);
        let mut sub = vec![wtype_name(wp.wtype, l).to_string(), format!("+{}", wp.level), RAR_NAME[l][wp.rar.min(3)].to_string()];
        if !wp.skill.is_empty() {
            sub.push(format!("{}{}", self.t("战技：", "Skill: "), wp.skill));
        }
        sub.push(format!("{}{:.1}", self.t("重量 ", "Weight "), wp.weight));
        d.text_clip(x + 20.0, y + 44.0, w - 40.0, 19.0, DIM, 1.0, &sub.join("  ·  "));
        // 两栏：左 攻击力 + 防御时减伤，右 能力加成 + 必需能力值
        let (ax, bx, cw) = (x + 16.0, x + 400.0, 340.0);
        let row = |d: &mut Draw, x: f32, y: f32, label: &str, val: &str, col: [u8; 3]| {
            d.text(x + 12.0, y, 21.0, TEXT, 0.9, label);
            let vw = d.width(val, 21.0);
            d.text(x + cw - 12.0 - vw, y, 21.0, col, 1.0, val);
        };
        let mut ay = y + 96.0;
        self.section(d, ax, ay, cw, self.t("攻击力", "Attack Power"));
        ay += 40.0;
        for e in 0..5 {
            let (b, bonus) = wp.atk[e];
            let (val, col) = match (b, bonus) {
                (0, _) => ("-".to_string(), DIM),
                (b, 0) => (b.to_string(), TEXT),
                (b, x) if x > 0 => (format!("{b} + {x}"), TEXT),
                (b, x) => (format!("{b} - {}", -x), ERRC),
            };
            row(d, ax, ay, ELEM[l][e], &val, col);
            ay += 30.0;
        }
        row(d, ax, ay, self.t("致命一击", "Critical"), &wp.crit.to_string(), TEXT);
        ay += 46.0;
        self.section(d, ax, ay, cw, self.t("防御时减伤", "Guarded Damage Negation"));
        ay += 40.0;
        for e in 0..5 {
            row(d, ax, ay, ELEM[l][e], &format!("{:.1}", wp.guard[e]), TEXT);
            ay += 30.0;
        }
        row(d, ax, ay, self.t("防御强度", "Guard Boost"), &wp.boost.to_string(), TEXT);
        let mut by = y + 96.0;
        self.section(d, bx, by, cw, self.t("能力加成", "Attribute Scaling"));
        by += 40.0;
        for s in 0..5 {
            let v = if wp.cor[s] > 0 { wp.cor[s].to_string() } else { "-".to_string() };
            // 超过原上限（超限）的金色
            let over = wp.ocap[s].is_some_and(|c| c[0] > c[2] + 1e-3);
            row(d, bx, by, STAT[l][s], &v, if over { [240, 200, 80] } else if wp.cor[s] > 0 { TEXT } else { DIM });
            by += 30.0;
        }
        by += 16.0;
        self.section(d, bx, by, cw, self.t("必需能力值", "Attributes Required"));
        by += 40.0;
        for s in 0..5 {
            let r = wp.req[s];
            let (v, col) = if r <= 0 { ("-".to_string(), DIM) } else if stats[s] < r { (r.to_string(), ERRC) } else { (r.to_string(), TEXT) };
            row(d, bx, by, STAT[l][s], &v, col);
            by += 30.0;
        }
        // 附加效果（整宽）
        let mut fy = ay.max(by) + 46.0;
        self.section(d, ax, fy, w - 32.0, self.t("附加效果", "Passive Effects"));
        fy += 40.0;
        if wp.fx.is_empty() {
            d.text(ax + 12.0, fy, 20.0, DIM, 1.0, "-");
        }
        for t in &wp.fx {
            if fy > y + 790.0 {
                break;
            }
            fy += d.wrapped(ax + 12.0, fy, w - 56.0, 20.0, TEXT, 0.92, t) + 4.0;
        }
    }

    /// 原版详情那种小标题：金字 + 下面一条线
    pub(super) fn section(&self, d: &mut Draw, x: f32, y: f32, w: f32, title: &str) {
        d.text(x + 4.0, y, 22.0, GOLD, 1.0, title);
        d.sprite("MENU_Line_01", x, y + 32.0, w, 3.0, 0.7);
    }

    // 右：重铸按钮、消耗、重铸后的范围
    fn draw_forge(&mut self, d: &mut Draw) {
        let (x, y, w) = (1430.0, 150.0, 420.0);
        let l = self.lang();
        d.text(x, y + 6.0, 26.0, GOLD, 1.0, self.t("重铸", "Reforge"));
        d.sprite("MENU_Line_01", x, y + 46.0, w, 3.0, 0.8);
        d.wrapped(x, y + 58.0, w, 19.0, DIM, 1.0, self.t(
            "重新抽取这把武器的能力补正（哪几项、多少）和能力需求，按它的稀有度、偏向你现在的加点；其他都不变。",
            "Rerolls this armament's attribute scaling (which and how much) and requirements by its rarity, leaning toward your current stats. Nothing else changes.",
        ));
        let Some(wp) = self.weapon() else { return };
        // 按钮
        let (bx, by, bw, bh) = (x, y + 140.0, w, 84.0);
        let focus = self.rcol == 1;
        let ok = self.stones_left() > 0 && !wp.ovl;
        if focus {
            d.sprite("MENU_FL_Cursor_Ent", bx, by, bw, bh, 1.0);
        }
        d.sprite("MENU_FL_Cursor_EntWaku", bx, by, bw, bh, if focus { 1.0 } else { 0.6 });
        let label = self.t("重铸", "Reforge");
        let lw = d.width(label, 30.0);
        // A 到这里、再 A 弹确认（Y 现在是下面的快速重铸）
        d.glyph("KG_XboxSeries_R_D", bx + (bw - lw) / 2.0 - 48.0, by + (bh - 38.0) / 2.0, 38.0);
        d.text_mid(bx + (bw - lw) / 2.0, by + bh / 2.0, 30.0, if ok { TEXT } else { DIM }, 1.0, label);
        self.hit(bx, by, bw, bh, Hit::Forge);
        // 快速重铸（Y / 键盘 R / 点）：不弹确认
        let (qy, qh) = (by + bh + 10.0, 60.0);
        d.sprite("MENU_FL_Cursor_EntWaku", bx, qy, bw, qh, 0.6);
        let ql = self.t("快速重铸", "Quick reforge");
        let qn = self.t("（不用确认）", " (no prompt)");
        let (qw, nw) = (d.width(ql, 24.0), d.width(qn, 18.0));
        let qx = bx + (bw - 40.0 - qw - nw) / 2.0;
        d.glyph("KG_XboxSeries_R_U", qx, qy + (qh - 32.0) / 2.0, 32.0);
        d.text_mid(qx + 40.0, qy + qh / 2.0, 24.0, if ok { TEXT } else { DIM }, 1.0, ql);
        d.text_mid(qx + 40.0 + qw, qy + qh / 2.0, 18.0, DIM, 1.0, qn);
        self.hit(bx, qy, bw, qh, Hit::QuickForge);
        // 消耗（超限过的不能重铸：这一行换成说明）
        let (s_icon, n) = self.snap.as_ref().map_or((0, 0), |s| (s.rstone_icon, s.rstones));
        let cy = qy + qh + 16.0;
        if wp.ovl {
            d.text_clip(x, cy + 9.0, w, 20.0, ERRC, 1.0, self.t("这把武器超限过，不能再重铸", "This armament has been overlimited and can't be reforged"));
            return;
        }
        d.icon(s_icon, x, cy, 40.0, 1.0);
        let cost = if l == 0 { format!("消耗 武器重铸石 ×1（现有 {n}）") } else { format!("Uses 1 Weapon Reforge Stone (have {n})") };
        d.text_clip_mid(x + 48.0, cy + 20.0, w - 48.0, 20.0, if ok { TEXT } else { ERRC }, 1.0, &cost);
        // 重铸后的范围
        let mut py = cy + 66.0;
        d.text(x, py, 22.0, GOLD, 1.0, self.t("重铸后（按现在的加点）", "After reforging (your current stats)"));
        d.sprite("MENU_Line_01", x, py + 32.0, w, 3.0, 0.7);
        py += 44.0;
        let cols = [x + 4.0, x + 80.0, x + 150.0, x + 240.0, x + 340.0];
        for (c, h) in cols.iter().zip([self.t("属性", "Stat"), self.t("现在", "Now"), self.t("几率", "Chance"), self.t("补正", "Scaling"), self.t("需求", "Req.")]) {
            d.text(*c, py, 18.0, DIM, 1.0, h);
        }
        py += 30.0;
        let pv = self.preview.clone().filter(|p| p.idx == wp.idx);
        for s in 0..5 {
            d.text(cols[0], py, 20.0, TEXT, 1.0, STAT[l][s]);
            let now = if wp.cor[s] > 0 { wp.cor[s].to_string() } else { "-".to_string() };
            d.text(cols[1], py, 20.0, if wp.cor[s] > 0 { TEXT } else { DIM }, 1.0, &now);
            match &pv {
                Some(p) => {
                    let (ch, c0, c1, r0, r1) = p.s[s];
                    let col = if ch > 0.0 { TEXT } else { DIM };
                    d.text(cols[2], py, 20.0, col, 1.0, &format!("{ch:.0}%"));
                    let span = |a: i32, b: i32| if ch <= 0.0 { "-".to_string() } else if a == b { a.to_string() } else { format!("{a}–{b}") };
                    d.text(cols[3], py, 20.0, col, 1.0, &span(c0, c1));
                    d.text(cols[4], py, 20.0, col, 1.0, &span(r0, r1));
                }
                None => d.text(cols[2], py, 20.0, DIM, 1.0, "…"),
            }
            py += 32.0;
        }
        if let Some(p) = &pv {
            py += 10.0;
            d.text(x, py, 19.0, DIM, 1.0, self.t("补正项数", "Scaling attributes"));
            py += 30.0;
            let parts: Vec<String> = (0..5).filter(|&i| p.n[i] >= 0.5).map(|i| if l == 0 { format!("{} 项 {:.0}%", i + 1, p.n[i]) } else { format!("{}: {:.0}%", i + 1, p.n[i]) }).collect();
            d.wrapped(x, py, w, 20.0, TEXT, 1.0, &parts.join("　"));
        }
    }

    pub(super) fn draw_reforge_confirm(&mut self, d: &mut Draw) {
        let Some(wp) = self.weapon() else { return };
        d.rect(-2000.0, -2000.0, 6000.0, 6000.0, [0, 0, 0], 0.5);
        d.band(-100.0, 400.0, 2120.0, 280.0, 0.92);
        d.sprite("MENU_FL_Dialog", -40.0, 410.0, 2000.0, 260.0, 0.8);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 418.0, 1600.0, 22.0, 0.9);
        d.sprite("MENU_FL_Cursor_Top", 160.0, 640.0, 1600.0, 22.0, 0.9);
        let l1 = if self.lang() == 0 { format!("重铸「{}」：重新抽取能力补正和能力需求", wp.name) } else { format!("Reforge \"{}\": reroll its scaling and requirements", wp.name) };
        let l2 = self.t("消耗 1 个武器重铸石。", "Uses 1 Weapon Reforge Stone.");
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
