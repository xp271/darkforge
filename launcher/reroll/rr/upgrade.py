# -*- coding: utf-8 -*-
"""生成武器的强化（weapon_upgrade，照 0.6.2 的 RNG 武器，用户 2026-10-29 要的）：最高 +5、每级 1 块石头，起点按稀有度；
dll\\dark_core 改写容器行时按「本体的强化类型 × 稀有度 × 是不是全能」换成这里建的强化类型、材料组。

- 成长（growth）：+k 相当于原版这个强化类型的 (k / 5 × growth × 满级) 级，满级是 +25（失色 +10），中间线性插值。
  传说 1.0（+5 = 原版满级）、史诗 0.8（≈ 传说 +4）、稀有 0.5（≈ 传说 +2.5）、普通 0.2（≈ 传说 +1）。
- 战技的固定伤害（baseAtkRate，如神圣刀刃的圣）、强化价格、maxReinforceLevel 不看稀有度：+5 都是原版满级（用户：满强和原版一致）；
  固定伤害的倍数另外压到 flat_max（2026-11-27 起 ×2，原版满级 ×4）。
- 全能（五项属性都有补正）和其他武器同一个强化类型（用户 2026-11-24「只用一个参数控制」：以前全能的补正成长只算 omni_scaling 0.583，
  照 0.6.2 的全能 ×1.35 / 其他 ×1.6；现在全能的打折只看 weapon_scaling.omni，dark_core 写参数行时乘，rr/affixes.py omni_mult）。
- 编号：强化类型 TYPE_BASE 起每个占 100（原版到 9010，reinforceTypeId 是 s16）；材料组 MAT_BASE + 稀有度 × 10 + 等级。
  原版的强化类型、材料组原版武器也在用，不动。
- 本体原版不能强化（强化类型只有 +0，陨石杖）的也按稀有度强化（用户 2026-10-31）：照同武器类型的本体最常用的失色强化类型
  （满级 +10，法杖是 2400）的行建；同类型没有失色的，照所有本体里最常用的失色强化类型。
- 已经强化到 +5 以上的（这之前拿到的）DLL 照原版强化类型。新强化类型的行也一直建到 +25（+rmax 以上照 +rmax 那行），
  换成新类型的武器不管几级都查得到强化行（2026-11-19）。
"""
from .common import *  # noqa: F401,F403

TYPE_BASE = 10000
MAT_BASE = 9900000
MAX_LV = 25                              # 原版最高强化等级：新强化类型的行一直建到这一级（+rmax 以上照 +rmax）
# 石头编号：锻造石 1~8 = 10100~10107，古龙岩 10140；失色 1~8 = 10160~10167，失色 9 = 10200，失色古龙岩 = 10168（不连号）
STONES = {
    'Common': [10100, 10101, 10102, 10103, 10104],
    'Rare': [10160, 10161, 10162, 10163, 10164],
    'Epic': [10104, 10105, 10106, 10107, 10140],
    'Legendary': [10165, 10166, 10167, 10200, 10168],
}
GROWTH = {'Common': 0.2, 'Rare': 0.5, 'Epic': 0.8, 'Legendary': 1.0}
# 看「满级」算的字段（不随稀有度打折）；整数字段（异常偏移等，原版这几类都是 0）照低一级那行
FULL_F = ('baseAtkRate', 'reinforcePriceRate')
# 战技固定伤害（baseAtkRate）+5 满级的倍数（用户 2026-11-27「下调至2倍」：战技补正改成跟武器以后固定伤害吃能力值多了，原版满级 ×4 降到 ×2）；
# +0 ×1 到 +5 ×flat_max 直线（原版几乎都是 +0 ×1 到满级 ×4 的直线：+1 ×1.2 … +5 ×2；强化类型 8600 的 +0 是 3.88、+1 起才从 1 涨，
# 照它等比压的话 +0 比 +1 还高，所以不照原版的形状）；0 = 照原版满级
FLAT_MAX = 2.0
CORRECT_F = ('correctStrengthRate', 'correctAgilityRate', 'correctMagicRate', 'correctFaithRate', 'correctLuckRate')
# 力敏的补正倍率提到和智力一样的强化类型（用户 2026-11-27「有些武器的全能为什么力敏的补正会略低」→「提到 ×1.8」）：
# 原版普通锻造石那条（0，本体 238 把）满级力敏 ×1.5、智信感 ×1.8——原版这类武器只有力敏补正，另外三项的倍数用不上；
# 生成武器五项都可能有，力敏就比另外三项低一截。失色那条（2200）等其他强化类型五项本来一样。每一级照同级的智力倍数
EVEN_STR_DEX = (0,)


def cfg(S):
    c = S.get('weapon_upgrade') or {}
    if not c or not c.get('enabled', True):
        return None
    stones = {r: [int(x) for x in (c.get('stones') or {}).get(r, STONES[r])] for r in RAR}
    if len({len(v) for v in stones.values()}) != 1 or not stones['Common']:
        raise RuntimeError('weapon_upgrade.stones：各稀有度的石头数要一样（= 最高强化等级）')
    return dict(stones=stones, growth={r: float((c.get('growth') or {}).get(r, GROWTH[r])) for r in RAR},
                max=len(stones['Common']), flat_max=float(c.get('flat_max', FLAT_MAX) or 0))


FULLWIDTH = str.maketrans('0123456789', '０１２３４５６７８９')


def stone_lines(c):
    """{石头: ([(稀有度, 强化到第几级)…])}：这块石头在生成武器上用在哪一级"""
    out = {}
    for r in RAR:
        for k, sid in enumerate(c['stones'][r]):
            out.setdefault(sid, []).append((r, k + 1))
    return out


def stone_texts(ctx, c):
    """锻造石的说明（用户 2026-11-07）：原版的「能将武器强化至＋３」换成这块石头在生成武器上的用法，
    「能将普通武器强化到＋５、史诗武器强化到＋１」（用户「不需要原版那句了 直接 能将【】武器强化到+【】」）；说明里那一行和一行简介都换"""
    uses = stone_lines(c)
    n = 0
    for code, lang in (('zhocn', 'zh'), ('engus', 'en')):
        mp = os.path.join(ctx.msg_out, code, 'item_dlc02.msgbnd.dcx')
        if not erlib.dcx_exists(mp):
            continue
        if lang == 'zh':
            key = '强化至＋'               # 锻造石「能将武器强化至」、失色锻造石「能将特殊武器强化至」
            say = {sid: '能将' + '、'.join('%s武器强化到＋%s' % (RAR_ZH[r], str(k).translate(FULLWIDTH)) for r, k in us) for sid, us in uses.items()}
            end = '。'
        else:
            key = 'Strengthens'
            say = {sid: 'Strengthens ' + ' and '.join('%s armaments to +%d' % (r, k) for r, k in us) for sid, us in uses.items()}
            end = '.'
        mb = erlib.Bnd4(erlib.read_dcx(mp))
        for fmg in ('GoodsCaption.fmg', 'GoodsInfo.fmg'):
            hdr, ents = erlib.read_fmg_full(mb.get(fmg))
            d = dict(ents)
            for sid, line in say.items():
                text = d.get(sid)
                if not text:
                    continue
                if fmg == 'GoodsInfo.fmg':
                    d[sid] = line
                else:
                    lines = text.split('\n')
                    k = next((x for x, l in enumerate(lines) if key in l), None)
                    if k is None:
                        lines += ['', line + end]
                    else:
                        lines[k] = line + end
                    d[sid] = '\n'.join(lines)
                n += 1
            mb.put(fmg, erlib.write_fmg(hdr, sorted(d.items())))
        erlib.write_dcx_file(mp, bytes(mb.data))
    ctx.report.append('锻造石说明：%d 种石头的「强化至」换成生成武器的用法（说明和一行简介，%d 条，中英文）' % (len(uses), n))


def build(ctx):
    ctx.upgrade = None
    c = cfg(ctx.S)
    if c is None or not getattr(ctx, 'containers', None):
        return
    lay = lambda x: {t[0]: t for t in pdm.layout(pdm.load(os.path.join(ctx.data_dir, x)))[0]}
    wl, rl, ml = lay('EquipParamWeapon.xml'), lay('ReinforceParamWeapon.xml'), lay('EquipMtrlSetParam.xml')
    g = lambda L, row, f: struct.unpack_from('<' + L[f][1], row, L[f][2])[0]

    def p(L, row, f, v):
        struct.pack_into('<' + L[f][1], row, L[f][2], v)

    W = erlib.Param(ctx.bnd.get('EquipParamWeapon.param'))
    RF = erlib.Param(ctx.bnd.get('ReinforceParamWeapon.param'))
    M = erlib.Param(ctx.bnd.get('EquipMtrlSetParam.param'))
    lib = json.load(open(os.path.join(ctx.data_dir, 'affixes', 'affixes.json'), encoding='utf-8'))
    wtypes = {int(t) for r in RAR for t in lib['weapon']['types'][r]}
    # 能当本体的原版武器（同 affixes.bases：编号整万、类型在词条库里）：（强化类型, 武器类型）
    bases = [(g(wl, row, 'reinforceTypeId'), g(wl, row, 'wepType')) for i, row in W.rows.items()
             if i < 100000000 and not i % 10000 and g(wl, row, 'wepType') in wtypes]
    used = sorted({t for t, _ in bases})
    floats = [f for f, t in rl.items() if t[1] == 'f']
    n_lv, kinds, rows_new, rows_pad = c['max'], [], 0, 0

    def top_of(t):
        k = 0
        while t + k + 1 in RF.rows and k < 25:
            k += 1
        return k
    tops = {t: top_of(t) for t in used}

    def somber(pick):
        """武器类型 pick(wt) 为真的本体里最常用的失色强化类型（满级不到 +25）"""
        n = Counter(t for t, wt in bases if pick(wt) and 0 < tops[t] < 25)
        return n.most_common(1)[0][0] if n else None
    fallback = []
    for t in used:
        src = t
        if tops[t] == 0:                                # 原版不能强化：照同武器类型的失色强化类型建
            wts = {wt for tt, wt in bases if tt == t}
            src = somber(lambda wt: wt in wts) or somber(lambda wt: True)
            if src is None:
                continue
            fallback.append((t, src))
        kinds.append((t, src, tops[src]))               # (原版强化类型, 照哪个强化类型的行建, 它的满级)

    def zero_cor(t):
        return all(g(rl, RF.rows[t], f) == 0 for f in CORRECT_F)

    def cor_ref(top):
        """满级同样是 top、补正倍率不是 0 的强化类型里，本体最多的那个"""
        n = Counter(t for t, wt in bases if tops[t] == top and not zero_cor(t))
        return n.most_common(1)[0][0] if n else None
    if TYPE_BASE + len(kinds) * len(RAR) * 100 > 32700:
        raise RuntimeError('强化类型太多，编号超出 s16')

    def level_row(t, top, e):
        """原版强化类型 t 在第 e 级（可以是小数）：两头的行线性插值"""
        lo = min(int(e), top)
        hi = min(lo + 1, top)
        w = e - lo
        a, b = RF.rows[t + lo], RF.rows[t + hi]
        return {f: g(rl, a, f) + (g(rl, b, f) - g(rl, a, f)) * w for f in floats}, a

    mapping, cor_fixed = [], []
    k_type = TYPE_BASE
    for t, src_t, top in kinds:
        # 补正倍率全是 0 的（原版的弩 3500、弩炮 / 大炮 3100~3400：本体没有补正）：生成的武器抽到了补正也被乘成 0，一点不生效
        # （用户 2026-11-01：多击发弩、壶大炮没有补正）。补正倍率照同满级的普通强化类型（cor_ref），别的照旧
        ref = cor_ref(top) if zero_cor(src_t) else None
        if ref is not None:
            cor_fixed.append((t, ref))
        for ri, r in enumerate(RAR):
            nt = k_type
            k_type += 100
            for lv in range(n_lv + 1):
                if nt + lv in RF.rows:
                    raise RuntimeError('强化类型 %d 已经有人用了' % (nt + lv))
                full = top * lv / n_lv
                vals, src = level_row(src_t, top, full * c['growth'][r])
                if ref is not None:
                    rv, _ = level_row(ref, tops[ref], full * c['growth'][r] * tops[ref] / top)
                    vals.update({f: rv[f] for f in CORRECT_F})
                if (src_t if ref is None else ref) in EVEN_STR_DEX:
                    for f in CORRECT_F[:2]:
                        vals[f] = max(vals[f], vals['correctMagicRate'])
                full_vals, full_src = level_row(src_t, top, full)
                if c['flat_max'] > 0:
                    full_vals['baseAtkRate'] = 1 + (c['flat_max'] - 1) * lv / n_lv
                row = bytearray(src)
                for f in floats:
                    p(rl, row, f, full_vals[f] if f in FULL_F else vals[f])
                p(rl, row, 'maxReinforceLevel', g(rl, full_src, 'maxReinforceLevel'))
                p(rl, row, 'materialSetId', lv)
                RF.add_row(nt + lv, row)
                rows_new += 1
            # +rmax 以上到 +25 照 +rmax 那行补上（崩溃风险审查 2026-11-19 第 10 条）：联机收到的、以前强化过头的武器换成这个强化类型时
            # 也查得到强化行；强化上限照旧由 dark_core 的 cap_origin 管（originEquipWep(rmax+1..25) = -1），铁匠不会再往上强化
            for lv in range(n_lv + 1, MAX_LV + 1):
                if nt + lv in RF.rows:
                    raise RuntimeError('强化类型 %d 已经有人用了' % (nt + lv))
                RF.add_row(nt + lv, bytes(RF.rows[nt + n_lv]))
                rows_pad += 1
            # 全能和其他武器用同一个强化类型（用户 2026-11-24：全能的补正打折只看 weapon_scaling.omni，强化成长不再另外打折）；
            # reinf 行照旧分全能（dark_core 按 (原版强化类型, 稀有度, 全能) 查）
            mapping += [(t, ri, 0, nt), (t, ri, 1, nt)]
    # 材料组：每级 1 块；拿原版「1 块失色锻造石」那行当模板（只第一格有材料）
    tmpl = next((M.rows[k] for k in sorted(M.rows) if g(ml, M.rows[k], 'materialId01') == 10160 and g(ml, M.rows[k], 'itemNum01') == 1
                 and g(ml, M.rows[k], 'materialId02') <= 0), None)
    if tmpl is None:
        raise RuntimeError('找不到强化材料行模板（1 块失色锻造石）')
    mats = []
    for ri, r in enumerate(RAR):
        m = MAT_BASE + ri * 10
        for lv, stone in enumerate(c['stones'][r], 1):
            if m + lv in M.rows:
                raise RuntimeError('材料组 %d 已经有人用了' % (m + lv))
            row = bytearray(tmpl)
            p(ml, row, 'materialId01', stone)
            p(ml, row, 'itemNum01', 1)
            M.add_row(m + lv, row)
        mats.append(m)
    ctx.bnd.put('ReinforceParamWeapon.param', RF.serialize())
    ctx.bnd.put('EquipMtrlSetParam.param', M.serialize())
    ctx.upgrade = dict(map=mapping, mats=mats, max=n_lv)
    stone_texts(ctx, c)
    ctx.report.append('生成武器的强化：最高 +%d、每级 1 块石头；%d 种原版强化类型 × 稀有度 → %d 个强化类型（全能共用）（%d 行，另 +%d~+%d 照 +%d 补 %d 行），满级成长 %s%s%s' % (
        n_lv, len(kinds), len(mapping) // 2, rows_new, n_lv + 1, MAX_LV, n_lv, rows_pad, '、'.join('%s %g' % (RAR_ZH[r], c['growth'][r]) for r in RAR),
        '；原版不能强化的强化类型照失色的建：%s' % '、'.join('%d ← %d' % x for x in fallback) if fallback else '',
        '；原版补正倍率是 0 的（弩、弩炮）补正倍率照：%s' % '、'.join('%d ← %d' % x for x in cor_fixed) if cor_fixed else ''))


def def_lines(ctx):
    """affixes.txt 里的强化：rmax 最高等级；rmat 各稀有度的材料组；reinf 原版强化类型、稀有度、全能、换成的强化类型"""
    u = getattr(ctx, 'upgrade', None)
    if not u:
        return []
    L = ['rmax\t%d' % u['max'], 'rmat\t' + '\t'.join(str(m) for m in u['mats'])]
    L += ['reinf\t%d\t%d\t%d\t%d' % x for x in u['map']]
    return L
