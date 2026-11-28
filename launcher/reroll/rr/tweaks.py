# -*- coding: utf-8 -*-
"""参数小改动：赐福记忆不丢卢恩、双子老妪卖卢恩弯弧、战技固定伤害倍数、道具伤害统一、提灯更亮、星光碎片随身上限。"""
from .common import *  # noqa: F401,F403


# 官方新版 regulation 里改了、我们的底子（1.17.1，随机器带的原版 regulation）还没有的：(参数, 行, 字段, 原来的值, 改成, 说明)。
# 用户 2026-11-15「补上」：1.17.3（游戏 1.17.2 带的，版本 11730034）和 1.17.1 逐表比，结构一样，内容只差这一个字段。
# 只在值还是原来的时候改（别的步骤或以后换了底子就不动）
OFFICIAL_FIXES = [
    ('Bullet', 20003309, 'atkId_Bullet', 0, 1, '1.17.3：兽骨诱惑箭（附箭羽）命中点留下的区域不再命中任何人（原来敌人、友方、自己都会被它命中）'),
]
OFFICIAL_XML = {'Bullet': 'BulletParam.xml'}


def official_fixes(ctx):
    import paramdef_min as pdm
    done = []
    for param, rid, field, old, new, why in OFFICIAL_FIXES:
        lay = pdm.layout(pdm.load(os.path.join(ctx.data_dir, OFFICIAL_XML[param])))[0]
        o, fc = next((off, fc) for nm, fc, off, bp, bits, d in lay if nm == field and not bits)
        P = erlib.Param(ctx.bnd.get(param + '.param'))
        row = P.rows.get(rid)
        if row is None or struct.unpack_from('<' + fc, row, o)[0] != old:
            continue
        struct.pack_into('<' + fc, row, o, new)
        ctx.bnd.put(param + '.param', P.serialize())
        done.append(why)
    ctx.report.append('官方新版的修正：%s' % ('；'.join(done) if done else '没有要补的（底子已经是新版）'))


def memory_of_grace(ctx):
    """赐福记忆（道具 115）用效果 3226 传回最后休息的赐福，这个效果带“清空所持卢恩”：去掉，传回去卢恩不丢"""
    if not ctx.S.get('memory_of_grace_keep_runes', True):
        return
    P = erlib.Param(ctx.bnd.get('SpEffectParam.param'))
    row = P.rows.get(3226)
    if row is None or not row[353] & 0b10:
        return
    row[353] &= ~0b10                         # clearSoul（第 353 字节第 1 位）
    ctx.bnd.put('SpEffectParam.param', P.serialize())
    ctx.report.append('赐福记忆：传回赐福不再丢卢恩')


SHARD = 1290                         # 星光碎片（道具）：原版随身最多 10 个、仓库 999 个


def shard_cap(ctx):
    """星光碎片随身最多带几个（starlight_shard_max，默认 99；用户 2026-11-13「修改星光碎片的持有上限到99」）。
    暗黑商店买魔法 / 祷告、斗技场试炼要用，Boss 也多给；原版随身 10 个，多出来的进仓库。仓库上限 999 不动；0 = 照原版"""
    n = int(ctx.S.get('starlight_shard_max', 99))
    if n <= 0:
        return
    import paramdef_min as pdm
    lay = pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'EquipParamGoods.xml')))[0]
    o, fc = next((off, fc) for nm, fc, off, bp, bits, d in lay if nm == 'maxNum' and not bits)
    G = erlib.Param(ctx.bnd.get('EquipParamGoods.param'))
    row = G.rows.get(SHARD)
    if row is None:
        ctx.report.append('星光碎片随身上限：没有道具 %d，没改' % SHARD)
        return
    old = struct.unpack_from('<' + fc, row, o)[0]
    n = min(n, 999)
    struct.pack_into('<' + fc, row, o, n)
    ctx.bnd.put('EquipParamGoods.param', G.serialize())
    ctx.report.append('星光碎片随身上限：%d → %d' % (old, n))


# 提灯（携带照明灯，道具 2070 → 效果 3245）的光源光效行和我们的光源特效（dev\item_system\sfx\lantern.js：原版 302421 只改衰减，
# 放在我们的 sfxbnd_c0000）；原版只有效果 3245 用这一行
LANTERN_VFX, LANTERN_SFX_VANILLA, LANTERN_SFX = 5401, 302421, 990060


def lantern_light(ctx):
    """提灯照得更亮（lantern_brighter，用户 2026-11-10「改一下油灯的亮度 参考 golden age 不需要改灯光颜色和位置」）：
    光效行 5401 的持续特效 302421 → 990060。990060 = 原版点光源，只把衰减（unk_ds3_p2_0）从 1.25 改成 Golden Age 幽魂提灯的 2，
    颜色、半径、亮度曲线、挂点（160 腰间）都不变；灯的模型和火苗（光效行 5400 / 特效 302420）不动"""
    if not ctx.S.get('lantern_brighter', True):
        return
    import paramdef_min as pdm
    ls, _ = pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'SpEffectVfx.xml')))
    off = next(o for nm, fc, o, bp, bits, d in ls if nm == 'midstSfxId')
    V = erlib.Param(ctx.bnd.get('SpEffectVfxParam.param'))
    row = V.rows.get(LANTERN_VFX)
    if row is None or struct.unpack_from('<i', row, off)[0] != LANTERN_SFX_VANILLA:
        ctx.report.append('提灯更亮：光效行 %d 不是原版的特效 %d，没改' % (LANTERN_VFX, LANTERN_SFX_VANILLA))
        return
    struct.pack_into('<i', row, off, LANTERN_SFX)
    ctx.bnd.put('SpEffectVfxParam.param', V.serialize())
    ctx.report.append('提灯更亮：光效行 %d 的光源换成 %d（衰减照 Golden Age）' % (LANTERN_VFX, LANTERN_SFX))


LANTERN_SP = 3245


def lantern_persist(ctx):
    """提灯传送、读图后不熄（lantern_persist）：效果 3245 的 saveCategory 从 -1 改成 0，和 ErdTools 的 persistent_lantern 改的是同一处。
    以前交给 ErdTools 做：它在游戏加载参数时自己去扫参数仓库找 SpEffectParam，有的电脑上仓库还没加载完，读到半成品的条目就崩
    （玩家崩溃诊断包 2026-11-18：敌人随机模式开游戏约 10 秒崩在 ErdTools.dll+0x13D43；用户「改」）。
    现在在参数里改好，启动器每次启动把 erd_tools.ini 的 persistent_lantern 设成 0（dfl\launch.py test_user_dll_safe）"""
    if not ctx.S.get('lantern_persist', True):
        return
    import paramdef_min as pdm
    ls, _ = pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'SpEffect.xml')))
    off = next(o for nm, fc, o, bp, bits, d in ls if nm == 'saveCategory')
    S = erlib.Param(ctx.bnd.get('SpEffectParam.param'))
    row = S.rows.get(LANTERN_SP)
    if row is None:
        ctx.report.append('提灯传送后保留：没有效果 %d，没改' % LANTERN_SP)
        return
    old = struct.unpack_from('<b', row, off)[0]
    if old == 0:
        ctx.report.append('提灯传送后保留：效果 %d 本来就会保留' % LANTERN_SP)
        return
    struct.pack_into('<b', row, off, 0)
    ctx.bnd.put('SpEffectParam.param', S.serialize())
    ctx.report.append('提灯传送后保留：效果 %d 的 saveCategory %d → 0（代替 ErdTools 的 persistent_lantern）' % (LANTERN_SP, old))


TWIN_SHOP = (101800, 101897)        # 双子老妪「购买」打开的商店段（赐福「双子老妪」→「购买」、圆桌厅堂都是这一段）


RUNE_ARC = 190


def twin_rune_arc(ctx):
    """双子老妪加卖卢恩弯弧（twin_rune_arc）：不限量，每个 price 卢恩，一开始就有（不看铃珠）。
    放在这一段第一个空号上（这一段原来的那格 4000 卢恩限购 5 个不动）；要在商店重抽、统一定价之后做，免得被换掉或改价"""
    cfg = ctx.S.get('twin_rune_arc') or {}
    if not cfg or not cfg.get('enabled', True):
        return
    import paramdef_min as pdm
    ls, _ = pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'ShopLineupParam.xml')))
    offs = {nm: (off, fc) for nm, fc, off, bp, bits, d in ls if not bits}
    P = erlib.Param(ctx.bnd.get('ShopLineupParam.param'))
    lo, hi = TWIN_SHOP
    free = [r for r in range(lo, hi + 1) if r not in P.rows]
    tmpl = next((r for r in range(lo, hi + 1) if r in P.rows), None)
    if not free or tmpl is None:
        ctx.report.append('双子老妪卢恩弯弧：商店段 %d~%d 没有空号，没加上' % TWIN_SHOP)
        return
    price = int(cfg.get('price', 10000))
    row = bytearray(P.rows[tmpl])
    for f, v in (('equipId', RUNE_ARC), ('value', price), ('mtrlId', -1), ('eventFlag_forStock', 0), ('eventFlag_forRelease', 0),
                 ('sellQuantity', -1), ('equipType', 3), ('costType', 0), ('setNum', 1), ('value_Add', 0), ('value_Magnification', 1.0),
                 ('iconId', -1), ('nameMsgId', -1), ('menuTitleMsgId', -1), ('menuIconId', -1)):
        struct.pack_into('<' + offs[f][1], row, offs[f][0], v)
    P.add_row(free[0], row)
    ctx.bnd.put('ShopLineupParam.param', P.serialize())
    ctx.report.append('双子老妪卢恩弯弧：商店行 %d，每个 %d 卢恩，不限量' % (free[0], price))


# 战技的固定伤害（skill_flat_damage_mult）：攻击参数里不看武器面板的那部分（atkPhys / Mag / Fire / Thun / Dark，
# 如黄金波动每段圣 250、神圣刀刃的圣）乘倍数，原版武器和 RNG 武器都吃（RNG 武器还另外乘强化的 baseAtkRate）。
# 只动战技（BehaviorParam_PC 3 亿段的行为，顺着子弹 / 命中后子弹 / 间隔生成子弹找下去）能到的攻击参数；
# 和法术 / 道具 / 普通攻击共用的行不动（2026-10-25 查过：205 行带固定伤害，一行共用的都没有）
SKILL_FLAT = ('atkPhys', 'atkMag', 'atkFire', 'atkThun', 'atkDark')


def skill_flat_damage(ctx):
    m = float(ctx.S.get('skill_flat_damage_mult', 1.0) or 1.0)
    if m == 1.0:
        return
    lay = lambda n: {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, n + '.xml')))[0] if not bits}
    bl, ul, al = lay('BehaviorParam'), lay('BulletParam'), lay('AtkParam')
    get = lambda o, row, n: struct.unpack_from('<' + o[n][1], row, o[n][0])[0]
    BP, BU, AT = (erlib.Param(ctx.bnd.get(n + '.param')) for n in ('BehaviorParam_PC', 'Bullet', 'AtkParam_Pc'))

    def atks(row):
        rt, ref = get(bl, row, 'refType'), get(bl, row, 'refId')
        out, seen, todo = set(), set(), []
        if rt == 0:
            out.add(ref)
        elif rt == 1:
            todo.append(ref)
        while todo:
            b = todo.pop()
            if b in seen or b not in BU.rows:
                continue
            seen.add(b)
            out.add(get(ul, BU.rows[b], 'atkId_Bullet'))
            todo += [v for v in (get(ul, BU.rows[b], 'HitBulletID'), get(ul, BU.rows[b], 'intervalCreateBulletId')) if v > 0]
        return {a for a in out if a > 0 and a in AT.rows}
    skill, other = set(), set()
    for rid, row in BP.rows.items():
        (skill if 300000000 <= rid < 400000000 else other).update(atks(row))
    flat = sorted(a for a in skill if any(get(al, AT.rows[a], f) for f in SKILL_FLAT))
    shared = [a for a in flat if a in other]
    for a in flat:
        if a in other:
            continue
        row = AT.rows[a]
        for f in SKILL_FLAT:
            v = get(al, row, f)
            if v:
                struct.pack_into('<' + al[f][1], row, al[f][0], min(65535, int(round(v * m))))
    ctx.bnd.put('AtkParam_Pc.param', AT.serialize())
    ctx.report.append('战技固定伤害 ×%g：%d 行攻击参数（只算战技用到的%s）' % (
        m, len(flat) - len(shared), '；和法术 / 道具 / 普通攻击共用的 %d 行没动' % len(shared) if shared else ''))


# 分元素的招式（用户 2026-11-17「按照原版倍率和基础面板计算随机后的倍率」「战技也按同样思路修改」）：
# 蕾拉娜的对剑原版两把剑各打一半元素——左手的魔力动作值是 0、右手的火是 0（面板 物 103 / 魔 66 / 火 66，每刀只吃到 169）；
# 专属战技里也有只吃一种元素的判定（夜与火之剑的魔力光束、火焰横扫，黄金律法大剑的圣光等）。生成武器的属性重新分配以后，
# 属性对不上的就打不出伤害（异常照常积累），对得上的又吃满整个面板（纯魔的夜与火光束比原版高三倍多）。
# 这种判定的五项动作值统一成 Σ(原武器的原版面板 × 原版动作值) ÷ 面板总和：原武器原版属性时每下伤害不变，
# 换了属性、或者战灰装到别的武器上，都照同一个比例吃面板。固定攻击力（atkPhys 这些）不乘动作值，不动。
# 只看抽属性的武器（weapon_element.skip_types 以外）自己的普通攻击（行为 1VVVJJJ）和专属战技（3VVVJJJ），顺着子弹找；
# 通用战灰（共享层，变体 0）没有原武器的面板，不动；被别的武器的行为也用到的攻击行不动；
# 原版这一下的武器攻击力部分本来就是 0 的（Σ = 0，只靠固定攻击力，像冰霜风暴）不动
ELEM_CORR = ('atkPhysCorrection', 'atkMagCorrection', 'atkFireCorrection', 'atkThunCorrection', 'atkDarkCorrection')
ELEM_BASE = ('attackBasePhysics', 'attackBaseMagic', 'attackBaseFire', 'attackBaseThunder', 'attackBaseDark')
ELEM_LAYERS = ((1, '普通攻击'), (3, '专属战技'))


def split_element_moves(ctx):
    we = ctx.S.get('weapon_element') or {}
    if not we.get('enabled', True):
        return
    skip = set(int(t) for t in we.get('skip_types', ()))
    lay = lambda n: {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, n + '.xml')))[0] if not bits}
    wl, bl, ul, al = lay('EquipParamWeapon'), lay('BehaviorParam'), lay('BulletParam'), lay('AtkParam')
    get = lambda o, row, n: struct.unpack_from('<' + o[n][1], row, o[n][0])[0]
    W, BP, BU, AT = (erlib.Param(ctx.bnd.get(n + '.param')) for n in ('EquipParamWeapon', 'BehaviorParam_PC', 'Bullet', 'AtkParam_Pc'))

    def atks(row):
        rt, ref = get(bl, row, 'refType'), get(bl, row, 'refId')
        out, seen, todo = set(), set(), []
        if rt == 0:
            out.add(ref)
        elif rt == 1:
            todo.append(ref)
        while todo:
            b = todo.pop()
            if b <= 0 or b in seen or b not in BU.rows:
                continue
            seen.add(b)
            out.add(get(ul, BU.rows[b], 'atkId_Bullet'))
            todo += [get(ul, BU.rows[b], 'HitBulletID'), get(ul, BU.rows[b], 'intervalCreateBulletId')]
        return {a for a in out if a > 0 and a in AT.rows}
    users = defaultdict(set)                    # 攻击行 → 用到它的行为
    for rid, row in BP.rows.items():
        for a in atks(row):
            users[a].add(rid)
    panels = defaultdict(dict)                  # 行为变体 → {本体: 原版面板}（抽属性的类型）
    all_vars = set()                            # 所有本体（含不抽属性的类型）的变体
    for wid, row in W.rows.items():
        if wid % 10000:
            continue
        var = get(wl, row, 'behaviorVariationId')
        all_vars.add(var)
        if var > 0 and get(wl, row, 'wepType') not in skip:
            panels[var][wid] = tuple(get(wl, row, f) for f in ELEM_BASE)
    vof = lambda b: (b // 1000) % 100000
    done, shared, mixed = defaultdict(list), 0, []
    for var, ws in sorted(panels.items()):
        own = {b for k, _ in ELEM_LAYERS for b in range(k * 100000000 + var * 1000, k * 100000000 + var * 1000 + 1000) if b in BP.rows}
        hits = {}                               # 攻击行 → 第一次在哪一层见到
        for k, _ in ELEM_LAYERS:
            for b in sorted(x for x in own if x // 100000000 == k):
                for a in atks(BP.rows[b]):
                    c = [get(al, AT.rows[a], f) for f in ELEM_CORR]
                    if max(c) > 0 and min(c) == 0:
                        hits.setdefault(a, k)
        if not hits:
            continue
        wid = min(w for w, p in ws.items() if sum(p) > 0) if any(sum(p) > 0 for p in ws.values()) else None
        if wid is None:
            continue
        if len(set(ws.values())) > 1:
            mixed.append(wid)                   # 几把面板不同的本体共用一个变体：照编号最小的那把算
        base = ws[wid]
        tot = sum(base)
        n = Counter()
        for a, k in sorted(hits.items()):
            # 别的行为也用到：只有没有本体的变体（原版的空变体）不算别人
            if any(b not in own and vof(b) in all_vars for b in users[a]):
                shared += 1
                continue
            c = [get(al, AT.rows[a], f) for f in ELEM_CORR]
            v = int(round(sum(p * x for p, x in zip(base, c)) / tot))
            if v <= 0:
                continue
            for f in ELEM_CORR:
                struct.pack_into('<' + al[f][1], AT.rows[a], al[f][0], v)
            n[k] += 1
        for k, zh in ELEM_LAYERS:
            if n[k]:
                done[zh].append('%d（%d 行）' % (wid, n[k]))
    ctx.bnd.put('AtkParam_Pc.param', AT.serialize())
    ctx.report.append('分元素的招式（动作值按原武器的原版面板统一成一个）：%s%s%s' % (
        '；'.join('%s %s' % (zh, '、'.join(done[zh])) for _, zh in ELEM_LAYERS if done[zh]) or '没有',
        '；和别的武器共用的 %d 行没动' % shared if shared else '',
        '；几把面板不同的本体共用一个变体（照编号最小的算）：%s' % mixed if mixed else ''))


# 单个战技的动作值倍数（skill_mv_scale，用户 2026-11-27「单独削弱下发劲和多重投掷 最高总倍率变为现在0.5倍」）：
# 战技编号 → 它的攻击行（开发时按 c0000 动作包里这个战技的判定 → 行为 → 攻击行查的，这些行只有这个战技自己的行为在用）；
# 五项动作值（atk*Correction，吃武器面板的那部分）乘倍数，固定攻击力不动。武器大类自己的变体行（发劲 602420xxx，动作值本来是 0）不在里面。
# 要在 split_element_moves 之后（专属战技的动作值先统一）、范围放大和专属战技战灰复制行之前
SKILL_MV_ROWS = {
    4010: (600000001, 600000002, 600000006, 600000007),                          # 发劲：提前松手 220 / 120，按满 460 / 180
    4030: (600000120, 600000121, 600000122, 600000123, 600000128, 600000129),    # 多重投掷（战灰）：每把 88 + 落地 53、双手 70
    5510: (600001000, 600001001, 600001002, 600001003, 600001008, 600001009),    # 多重投掷（专属）：每把 70 + 落地 39、双手 70
}


def skill_mv_scale(ctx):
    conf = ctx.S.get('skill_mv_scale') or {}
    skills = {int(k): float(v) for k, v in (conf.get('skills') or {}).items()}
    if not conf.get('enabled', True) or not skills:
        return
    al = {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'AtkParam.xml')))[0] if not bits}
    AT = erlib.Param(ctx.bnd.get('AtkParam_Pc.param'))
    done, miss = [], []
    for sid, m in sorted(skills.items()):
        rows = [a for a in SKILL_MV_ROWS.get(sid, ()) if a in AT.rows]
        if not rows:
            miss.append(str(sid))
            continue
        for a in rows:
            row = AT.rows[a]
            for f in ELEM_CORR:
                v = struct.unpack_from('<' + al[f][1], row, al[f][0])[0]
                if v:
                    struct.pack_into('<' + al[f][1], row, al[f][0], min(65535, int(round(v * m))))
        done.append('%d ×%g（%d 行）' % (sid, m, len(rows)))
    ctx.bnd.put('AtkParam_Pc.param', AT.serialize())
    ctx.report.append('单个战技的动作值倍数：%s%s' % ('、'.join(done) or '没有', '；不认得的战技 %s' % '、'.join(miss) if miss else ''))


# 战技补正跟武器（skill_correct_follow_weapon，用户 2026-11-27「可以把战技的补正都改为跟武器吗」「直接改」）：
# 原版战灰的攻击行大多改写了属性补正表（overwriteAttackElementCorrectId → 51010~51120 这类：只吃一项能力值、补正值写死 25，
# 两项的各 15），装在什么武器上都一样；少数专属战技也有自己的表（约定之王圣←智、兽王爪、尸蜡斩击不吃能力值等）。
# 清成 -1 = 按武器自己的属性补正表和补正值算（生成武器是 42300：五种伤害都吃武器有补正的几项）。
# 战技 = 行为 3 亿段（本体）和 6 亿段（黄金树幽影），顺着子弹 / 命中后子弹 / 间隔生成子弹找；和普通攻击 / 法术 / 道具共用的行不动。
# 在范围放大、专属战技战灰之前（复制的行带上改过的）
def skill_correct_follow_weapon(ctx):
    if not ctx.S.get('skill_correct_follow_weapon', True):
        return
    lay = lambda n: {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, n + '.xml')))[0] if not bits}
    bl, ul, al = lay('BehaviorParam'), lay('BulletParam'), lay('AtkParam')
    get = lambda o, row, n: struct.unpack_from('<' + o[n][1], row, o[n][0])[0]
    BP, BU, AT = (erlib.Param(ctx.bnd.get(n + '.param')) for n in ('BehaviorParam_PC', 'Bullet', 'AtkParam_Pc'))

    def atks(row):
        rt, ref = get(bl, row, 'refType'), get(bl, row, 'refId')
        out, seen, todo = set(), set(), []
        if rt == 0:
            out.add(ref)
        elif rt == 1:
            todo.append(ref)
        while todo:
            b = todo.pop()
            if b in seen or b not in BU.rows:
                continue
            seen.add(b)
            out.add(get(ul, BU.rows[b], 'atkId_Bullet'))
            todo += [v for v in (get(ul, BU.rows[b], 'HitBulletID'), get(ul, BU.rows[b], 'intervalCreateBulletId')) if v > 0]
        return {a for a in out if a > 0 and a in AT.rows}
    skill, other = set(), set()
    for rid, row in BP.rows.items():
        (skill if rid // 100000000 in (3, 6) else other).update(atks(row))
    f = 'overwriteAttackElementCorrectId'
    own = sorted(a for a in skill if get(al, AT.rows[a], f) > 0)
    tables = Counter()
    shared = 0
    for a in own:
        if a in other:
            shared += 1
            continue
        tables[get(al, AT.rows[a], f)] += 1
        struct.pack_into('<' + al[f][1], AT.rows[a], al[f][0], -1)
    ctx.bnd.put('AtkParam_Pc.param', AT.serialize())
    ctx.report.append('战技补正跟武器：%d 行攻击参数不再用自己的补正表（%s）%s' % (
        sum(tables.values()), '、'.join('%d ×%d' % kv for kv in tables.most_common()) or '没有',
        '；和普通攻击 / 法术 / 道具共用的 %d 行没动' % shared if shared else ''))


# 道具补正（item_scaling_mult，用户 2026-11-05「修改所有道具的补正到现在的2.5倍」）：道具的能力补正在它的虚拟武器上
# （EquipParamGoods.refVirtualWepId → EquipParamWeapon，wepType 0，玩家拿不到），伤害 = 道具的固定攻击力 × (100 + Σ 补正值 × 曲线) ÷ 100，菜单上的补正也读它。
# 几个道具共用一行的只乘一次
ITEM_SCALE_FIELDS = ('correctStrength', 'correctAgility', 'correctMagic', 'correctFaith', 'correctLuck')


def lightning_ram(ctx):
    """雷电羊球（SwordArtsParam 504）、雷云化身（1026）按住一直放时只有按下第一下消耗专注值（用户 2026-11-09）：第一下走 L2 的费用，
    接着放走 R2 的费用（sc/hks.py DR_RamHold / DR_CloudReplay），R2 改成 0"""
    P = erlib.Param(ctx.bnd.get('SwordArtsParam.param'))
    L = {t[0]: t for t in pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'SwordArtsParam.xml')))[0]}
    for art in (504, 1026):
        row = P.rows.get(art)
        if row is not None:
            struct.pack_into('<' + L['useMagicPoint_R2'][1], row, L['useMagicPoint_R2'][2], 0)
    ctx.bnd.put('SwordArtsParam.param', P.serialize())


# 法杖 / 圣印记的流派加成（用户 2026-11-13「法杖圣印上各流派伤害加成统一为25%」）：词条库 weapon.resident 的 staff / seal 池
# 每种流派一个原版效果行（1910~1928、5335200、5345000~5345200、5011000，原版只有武器引用），五种攻击倍率改成 1 + pct / 100；
# 说明里的数字也是词条库的 pct（extract_affixes.py SCHOOL_PCT），两边一起变。原版法杖、圣印记挂的也是这些行，跟着变
SCHOOL_RATES = ('physicsAttackRate', 'magicAttackRate', 'fireAttackRate', 'thunderAttackRate', 'darkAttackRate')


def catalyst_school(ctx):
    lib = json.load(open(os.path.join(ctx.data_dir, 'affixes', 'affixes.json'), encoding='utf-8'))
    res = (lib.get('weapon') or {}).get('resident') or {}
    want = {x['sp']: x['pct'] for k in ('staff', 'seal') for x in (res.get(k) or {}).get('pool') or []}
    if not want:
        return
    lay = {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'SpEffect.xml')))[0] if not bits}
    S = erlib.Param(ctx.bnd.get('SpEffectParam.param'))
    n = 0
    for sp, pct in sorted(want.items()):
        row = S.rows.get(sp)
        if row is None:
            continue
        for f in SCHOOL_RATES:
            struct.pack_into('<' + lay[f][1], row, lay[f][0], 1.0 + pct / 100.0)
        n += 1
    ctx.bnd.put('SpEffectParam.param', S.serialize())
    ctx.report.append('法杖 / 圣印记流派加成：%d 个效果统一成 %s%%' % (n, '/'.join(sorted({'%g' % p for p in want.values()}))))


def item_scaling(ctx):
    m = float(ctx.S.get('item_scaling_mult', 1.0))
    if m == 1.0:
        return
    lay = lambda n: {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, n + '.xml')))[0] if not bits}
    gl, wl = lay('EquipParamGoods'), lay('EquipParamWeapon')
    get = lambda o, row, n: struct.unpack_from('<' + o[n][1], row, o[n][0])[0]
    G, W = erlib.Param(ctx.bnd.get('EquipParamGoods.param')), erlib.Param(ctx.bnd.get('EquipParamWeapon.param'))
    ids = {get(gl, row, 'refVirtualWepId') for row in G.rows.values()}
    n = 0
    for wid in sorted(ids):
        row = W.rows.get(wid)
        if wid <= 0 or row is None or get(wl, row, 'wepType') != 0:
            continue
        for f in ITEM_SCALE_FIELDS:
            struct.pack_into('<' + wl[f][1], row, wl[f][0], get(wl, row, f) * m)
        n += 1
    ctx.bnd.put('EquipParamWeapon.param', W.serialize())
    ctx.report.append('道具补正：%d 种道具用的虚拟武器的补正值 × %g' % (n, m))


# 道具伤害统一（item_damage_unify，用户 2026-11-01）：攻击子类别带 投掷壶 108、调香 109（含调香瓶武器）、飞刀 / 流纹武器投掷 119、
# DLC 大壶 131 的攻击行，和道具（EquipParamGoods 的 refId 直接是攻击 / 子弹）顺着子弹能到的攻击行（石头等原来没有子类别的也算），
# 都加上子类别 108。词条的「道具伤害」只认 108（dev\item_system\extract_affixes.py 的 ITEM_DMG），原版认 108 的效果也跟着管到这些。
# 道具能到、但武器 / 战技行为（1 亿以上）也用、又不带这几类的行不动（攻击 1 这种通用行）
ITEM_TAG = 108
ITEM_CATS = (108, 109, 119, 131)
SUB_CATS = ('subCategory1', 'subCategory2', 'subCategory3', 'subCategory4')


def item_damage(ctx):
    if not ctx.S.get('item_damage_unify', True):
        return
    lay = lambda n: {f: (o, fc) for f, fc, o, bp, bits, d in pdm.layout(pdm.load(os.path.join(ctx.data_dir, n + '.xml')))[0] if not bits}
    gl, bl, ul, al = lay('EquipParamGoods'), lay('BehaviorParam'), lay('BulletParam'), lay('AtkParam')
    get = lambda o, row, n: struct.unpack_from('<' + o[n][1], row, o[n][0])[0]
    G, BP, BU, AT = (erlib.Param(ctx.bnd.get(n + '.param')) for n in ('EquipParamGoods', 'BehaviorParam_PC', 'Bullet', 'AtkParam_Pc'))

    def atks(kind, ref):
        """kind 0 = 攻击、1 = 子弹（顺着命中后子弹 / 间隔生成子弹找下去）"""
        out, seen, todo = set(), set(), []
        if kind == 0:
            out.add(ref)
        elif kind == 1:
            todo.append(ref)
        while todo:
            b = todo.pop()
            if b <= 0 or b in seen or b not in BU.rows:
                continue
            seen.add(b)
            out.add(get(ul, BU.rows[b], 'atkId_Bullet'))
            todo += [get(ul, BU.rows[b], 'HitBulletID'), get(ul, BU.rows[b], 'intervalCreateBulletId')]
        return {a for a in out if a > 0 and a in AT.rows}
    goods, weapon = set(), set()
    for row in G.rows.values():
        for f in ('refId_default', 'refId_1'):
            if get(gl, row, f) > 0:
                goods |= atks(get(gl, row, 'refCategory'), get(gl, row, f))
    for rid, row in BP.rows.items():
        if rid >= 100000000:
            weapon |= atks(get(bl, row, 'refType'), get(bl, row, 'refId'))
    n = Counter()
    full = []
    for a, row in AT.rows.items():
        subs = [get(al, row, f) for f in SUB_CATS]
        if ITEM_TAG in subs:
            continue
        if set(subs) & set(ITEM_CATS):
            kind = '带道具类子类别'
        elif a in goods and a not in weapon:
            kind = '道具的'
        else:
            continue
        if 0 not in subs:
            full.append(a)
            continue
        f = SUB_CATS[subs.index(0)]
        struct.pack_into('<' + al[f][1], row, al[f][0], ITEM_TAG)
        n[kind] += 1
    ctx.bnd.put('AtkParam_Pc.param', AT.serialize())
    ctx.report.append('道具伤害统一：攻击参数加子类别 %d 共 %d 行（%s）；道具和武器 / 战技共用的 %d 行没动%s' % (
        ITEM_TAG, sum(n.values()), '、'.join('%s %d' % kv for kv in sorted(n.items())),
        len(goods & weapon), '；四个子类别位都满、加不上的 %d 行：%s' % (len(full), full[:10]) if full else ''))


SCALING_TABLE = range(100, 110)     # MenuValueTableParam 里格式码 100 那组：武器能力补正值 → Ｓ~Ｅ（第 100~106 行）


def scaling_numbers(ctx):
    """武器补正显示具体数值（weapon_scaling_numbers）：面板上补正那几格（MenuPropertySpecParam 的 FormatType 100）先按格式码
    查 MenuValueTableParam 同号那组，查到就显示字母；查不到才走代码里格式码 100 那一支，dll\\dark_core 把那一支改成印数字
    （scaling.rs，0 照旧「-」）。所以这里删掉那组行；没装 DLL 时那一支照样显示字母（门槛和表里的略有不同）"""
    if not ctx.S.get('weapon_scaling_numbers', True):
        return
    P = erlib.Param(ctx.bnd.get('MenuValueTableParam.param'))
    gone = [i for i in sorted(P.rows) if i in SCALING_TABLE]
    for i in gone:
        P.del_row(i)
    ctx.bnd.put('MenuValueTableParam.param', P.serialize())
    ctx.report.append('武器补正显示数值：删掉 MenuValueTableParam 第 %s 行（补正 → 字母），数字由 dll\\dark_core 印' % (
        '~'.join(str(x) for x in (gone[0], gone[-1])) if gone else '（本来就没有）'))
