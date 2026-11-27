# -*- coding: utf-8 -*-
"""武器重铸石 + 「武器回收」（以前是暗黑商店的「装备回收」）（settings.json 的 weapon_reforge；用户 2026-10-30）：
  - 道具 GOODS：强化材料类（照锻造石【1】，不能使用；图标借铁制砥石刀的）。用户 2026-10-31：只在装备工坊里重铸——
    赐福「暗黑重铸」→「装备工坊」的「重铸」页选背包里的一把生成武器，dll\\dark_core 按它的稀有度重抽能力补正和能力需求
    （和鉴定同一套，偏向当前加点；本体、战技、附加效果、词条、特效、强化等级都不变），扣一个。
    （原来是背包里直接用：照黄金卢恩【1】、使用时挂标记效果，重铸右手武器；那条路去掉了，标记效果 369180 / 369181 也不再建）
  - 赐福「暗黑重铸」里的「武器回收」（菜单在 sc/darkshop.py 建、sc/forge.py 收进来）：选档位（普通 / 稀有及以下 / 史诗及以下 / 传说及以下）→ 确认框
    （dark_core 按背包现状改写预计件数）→ 点亮请求标记 RECYCLE_FLAGS[档]；dark_core 把背包里（不含仓库）没装备着、没锁定的、
    这一档及以下的生成武器移除，每把按稀有度给 recycle_stones 个重铸石（借发货表发），回收掉的不进回购商店。只收武器（用户选的）。
DLL 用的定义写进 dll\\dark_core\\items.txt 的 reforge / recycle 行（lines，containers.write_def 调）；请求标记给 sc 的写在 features.json 的 recycle。"""
from .common import *  # noqa: F401,F403

GOODS = 9240                       # 道具编号（未鉴定物品 9200~9224 后面，原版没用）
TEMPLATE = 10100                   # 锻造石【1】：强化材料（goodsType 14）、不能使用
ICON_FROM = 8970                   # 铁制砥石刀的图标（生成的武器不能质变，砥石刀已经不掉了，不会认错；菜单包有自制的那份时 dark_core 换上）
RECYCLE_FLAGS = (93087, 93088, 93089, 93090)   # 回收请求：普通 / 稀有及以下 / 史诗及以下 / 传说及以下（93000 块的空号，dev\event_flags\说明.md）
RECYCLE_DISCARD_FLAG = 93096                   # 「回收全部弃置」（用户 2026-11-26）：超限页武器栏打了弃置标记的那些，按各自稀有度给；
                                               # 93094~93099 是原版闪烁提示迁到 92560 之后空出来的（dev\event_flags\说明.md）。
                                               # 最初用 93012，和许愿石请求 / 出售弃置撞了，2026-11-26 首次实测踩到
SORT = 160101                      # 排在融锻之心（160100）后面
DEFAULT_STONES = {'Common': 1, 'Rare': 2, 'Epic': 3, 'Legendary': 4}


def cfg(S):
    c = S.get('weapon_reforge') or {}
    if not c.get('enabled', True):
        return None
    st = dict(DEFAULT_STONES, **(c.get('recycle_stones') or {}))
    return {'stones': [max(0, int(st[r])) for r in RAR]}


def texts(c):
    """（名字, 简介, 说明）：中文、英文"""
    n = c['stones']
    zh = ('武器重铸石', '装备工坊「重铸」消耗的材料',
          '回收武器后残留的石头，仍蕴含让武器重新成形的力量。\n\n'
          '流浪民族的商人收购武器时，以此石抵付货款。\n\n'
          '在赐福「暗黑重铸」→「装备工坊」的「重铸」里，选背包里的一把新武器，重新抽取它的能力补正（哪几项、多少）和能力需求：'
          '按这把武器的稀有度，和鉴定时一样偏向你当前的加点。武器本体、战技、附加效果、词条、特效和强化等级都不变。每次消耗 1 个。\n\n'
          '在赐福「暗黑重铸」→「武器回收」回收新武器获得：普通 %d 个、稀有 %d 个、史诗 %d 个、传说 %d 个。' % tuple(n))
    en = ('Weapon Reforge Stone', 'Material used by Reforge in the Gear Workshop',
          'A stone left behind by recycled armaments, still holding the power to give a weapon a new shape.\n\n'
          'When the nomadic merchants buy armaments, they pay with these stones.\n\n'
          'At a site of grace, "Darkforge" -> "Gear Workshop" -> "Reforge" uses one to reroll the attribute scaling (which attributes '
          'and how much) and the requirements of a new armament in your inventory. The new values follow the armament\'s rarity '
          'and, as with identification, lean toward your current stats. The base weapon, skill, effects, affixes and reinforcement level '
          'stay the same.\n\n'
          'Obtained by recycling new armaments at a site of grace ("Darkforge" -> "Recycle Weapons"): '
          'Common %d, Rare %d, Epic %d, Legendary %d each.' % tuple(n))
    return {'zh': zh, 'en': en}


def build(ctx):
    ctx.reforge = None
    c = cfg(ctx.S)
    if not c or not getattr(ctx, 'containers', None):
        return
    gl, gsz = pdm.layout(pdm.load(os.path.join(ctx.data_dir, 'EquipParamGoods.xml')))
    go = {n: (o, fc, bp, bits) for n, fc, o, bp, bits, _ in gl}
    G = erlib.Param(ctx.bnd.get('EquipParamGoods.param'))
    if gsz != G.row_size or TEMPLATE not in G.rows or ICON_FROM not in G.rows:
        raise RuntimeError('武器重铸石：EquipParamGoods 行大小不对，或缺少锻造石【1】%d / 铁制砥石刀 %d' % (TEMPLATE, ICON_FROM))
    if GOODS in G.rows:
        raise RuntimeError('武器重铸石：道具编号 %d 已被占用' % GOODS)
    icon = struct.unpack_from('<' + go['iconId'][1], G.rows[ICON_FROM], go['iconId'][0])[0]
    row = bytearray(G.rows[TEMPLATE])
    for f, v in (('iconId', icon), ('sortId', SORT), ('rarity', 2), ('sellValue', 100), ('maxNum', 999), ('maxRepositoryNum', 999)):
        o, fc, bp, bits = go[f]
        if bits:
            raise RuntimeError('武器重铸石：%s 是位字段' % f)
        struct.pack_into('<' + fc, row, o, v)
    G.add_row(GOODS, bytes(row))
    ctx.bnd.put('EquipParamGoods.param', G.serialize())

    tx = texts(c)
    for code, lang in (('zhocn', 'zh'), ('engus', 'en')):
        mp = os.path.join(ctx.msg_out, code, 'item_dlc02.msgbnd.dcx')
        if not erlib.dcx_exists(mp):
            continue
        mb = erlib.Bnd4(erlib.read_dcx(mp))
        for j, fmg in enumerate(('GoodsName.fmg', 'GoodsInfo.fmg', 'GoodsCaption.fmg')):
            hdr, ents = erlib.read_fmg_full(mb.get(fmg))
            d = dict(ents)
            d[GOODS] = tx[lang][j]
            mb.put(fmg, erlib.write_fmg(hdr, sorted(d.items())))
        erlib.write_dcx_file(mp, bytes(mb.data))
    ctx.reforge = c
    ctx.features['recycle'] = {'flags': list(RECYCLE_FLAGS), 'stones': c['stones'], 'discard': RECYCLE_DISCARD_FLAG}
    ctx.report.append('武器重铸石：道具 %d（强化材料，装备工坊「重铸」消耗，dll\\dark_core 重抽生成武器的补正和需求）；'
                      '「武器回收」：生成武器每把给 %s 个（普通 / 稀有 / 史诗 / 传说），请求标记 %d~%d；'
                      '「回收全部弃置」的标记 %d'
                      % (GOODS, ' / '.join(map(str, c['stones'])), RECYCLE_FLAGS[0], RECYCLE_FLAGS[-1], RECYCLE_DISCARD_FLAG))


def lines(ctx):
    """items.txt 的重铸石、回收行"""
    c = getattr(ctx, 'reforge', None)
    if not c:
        return []
    return ['# reforge <道具> 0 0（后两个以前是使用时的标记效果、金光，背包里直接用的路去掉后不用了，留着格式）；'
            'recycle <道具> <请求标记 普通,稀有及以下,史诗及以下,传说及以下> <每把给几个 普通,稀有,史诗,传说> <回收全部弃置的标记>（rr/reforge.py）',
            'reforge\t%d\t0\t0' % GOODS,
            'recycle\t%d\t%s\t%s\t%d' % (GOODS, ','.join(map(str, RECYCLE_FLAGS)), ','.join(map(str, c['stones'])), RECYCLE_DISCARD_FLAG)]
