# -*- coding: utf-8 -*-
"""暗黑商店：赐福「更多选项」里原来的护符 / 魔法 / 祷告商店（选项 30 / 31 / 32，RNG Lands 留下的，魔法 / 祷告商店已经没东西卖）去掉，
换成「暗黑重铸」（sc/forge.py，2026-10-31 起）里的一项「暗黑商店」，子菜单里是 近战武器 / 远程武器 / 盾牌 / 护符 / 战灰 / 魔法 / 祷告 / 道具 / 回购 九个商店
（回购商店卖最后卖掉的生成装备，商品由 dll\\dark_core 按记录放）。
商店行是重随的 rr/darkshop.py 建的，features.json 的 dark_shop 给出各商店的行范围和页面标题（GR_MenuText）。
护符商店、战灰商店收龙心脏：照原版龙飨祭坛用龙飨商店界面打开（1:135，页面里显示持有的龙心脏；dark_shop.talisman_dragon_ui = false 就用普通商店界面），
魔法 / 祷告商店收星光碎片：照原版瑟濂卖傀儡用的界面打开（1:144，页面里显示持有的星光碎片；dark_shop.spell_shard_ui = false 就用普通商店界面），
武器 / 道具商店收卢恩，用普通商店界面（1:22）。没有重随包（没有 dark_shop）就只把原来那三项去掉。
战灰商店打败葛瑞克（features 的 dark_shop.ash_release，分阶段解锁时是第一个区域的追忆 Boss）；魔法 / 祷告商店（dark_shop.spell_release）用户 2026-11-05 改成一开始就有（0 = 不看标记，基础法术开局就卖，其余按区域，rr/darkshop.py）；商店行各自按区域上架
才在子菜单里出现（AddTalkListDataIf），商店行本身也带同一个上架标记。
「武器回收」（以前叫「装备回收」、在这个子菜单最后；用户 2026-10-31 挪进「暗黑重铸」、改名，状态机还在这里建，
用 register 登记；features.json 有 recycle 才加，重随的 rr/reforge.py；用户 2026-10-30）：选档位（普通 / 稀有及以下 / 史诗及以下 /
传说及以下）→ 确认框（「〔#I3 …〕」~「〔#I6 …〕」占位，dll\\dark_core 按背包现状改写预计件数）→ 点亮请求标记、弹「已回收」；
dark_core 把这一档及以下的生成武器换成武器重铸石。提示框关掉时标记还亮着 = DLL 没加载：熄灭标记，改弹「没有加载 dark_core」"""
from .esdutil import ALWAYS, DIALOG_WAIT, GRACE_WAIT, MENU_WAIT, flag_is_on, i32, menu_parts, menu_pick, option_of, register
from .sell import DIGITS, TIERS, TIERS_EN

OPTION, TEXT = 35, 80090004
OLD = (30, 31, 32)                 # 「更多选项」里原来的护符 / 魔法 / 祷告商店
SHOPS = (('melee', 80090008), ('ranged', 80090009), ('shield', 80090010), ('talisman', 80090001), ('ash', 80090019),
         ('sorcery', 80090050), ('incant', 80090051), ('item', 80090007), ('buyback', 80090018))     # 子菜单的顺序和文字编号
DRAGON_SHOPS = ('talisman', 'ash')                 # 收龙心脏的：龙飨祭坛的界面（dark_shop.talisman_dragon_ui）
OPEN_REGULAR, OPEN_DRAGON, OPEN_SHARD = 22, 135, 144                         # 打开商店的命令（1:22 普通 / 1:135 龙飨 / 1:144 瑟濂的傀儡）
MENU_REGULAR, MENU_DRAGON, MENU_SHARD = 5, 22, 28                            # 对应的界面类型（等它关掉）
NAMES = {'zh': {'menu': '暗黑商店', 'melee': '近战武器商店', 'ranged': '远程武器商店', 'shield': '盾牌商店', 'talisman': '护符商店',
                'ash': '战灰商店', 'sorcery': '魔法商店', 'incant': '祷告商店', 'item': '道具商店', 'buyback': '回购商店'},
         'en': {'menu': 'Dark Shop', 'melee': 'Melee Armaments', 'ranged': 'Ranged Weapons / Catalysts', 'shield': 'Shields',
                'talisman': 'Talisman Shop', 'ash': 'Ashes of War', 'sorcery': 'Sorceries', 'incant': 'Incantations', 'item': 'Item Shop',
                'buyback': 'Buyback'}}
# 武器回收：菜单项、档位名 4 个、确认框 4 个、已回收、没加载 DLL；确认框占位的种类 3~6（0 一键鉴定、1 / 2 许愿，dark_core 的 confirm_tick）
RECYCLE_TEXT, R_TIER_TEXT, R_DESC_TEXT, R_DONE_TEXT, R_MISSING_TEXT = 80090200, 80090201, 80090205, 80090209, 80090210
R_KIND0 = 3
# 「回收全部弃置」（用户 2026-11-26）：档位名、确认框（80090220 段空着；不能接在 R_DESC_TEXT 后面，那个会撞 R_DONE_TEXT）
# 确认框占位序号 18（#II）：dark_core 的 confirm_tick 第 18 项（17 #IH 是许愿石的结果文字，20~26 是斗技场的，别撞）
R_DISCARD_TEXT, R_DISCARD_DESC, R_DISCARD_KIND = 80090220, 80090221, 18
FIELD_LEN = {'zh': 30, 'en': 44}                   # 含两端括号的字符数（和一键鉴定一样）


def field(kind, lang):
    if lang == 'zh':
        body = '#I%s 正在清点背包……' % DIGITS[kind]
        return '〔' + body + ' ' * (FIELD_LEN['zh'] - 2 - len(body)) + '〕'
    body = '#I%s counting...' % DIGITS[kind]
    return '[' + body + ' ' * (FIELD_LEN['en'] - 2 - len(body)) + ']'


def recycle_group(p, flags, discard=0):
    """装备回收的状态机：0 / 1 档位列表；200 + t 确认框；300 + t 点亮标记、弹提示；400 + t 看 DLL 有没有处理；500 + t 没处理；9 返回
    （用户 2026-11-26：第 5 项「回收全部弃置」的标记是 discard，0 = 没有；档位名和确认框文字用新编号）"""
    add_cmd, conf_call, yes_eval, no_eval, ret = (p[k] for k in ('add_cmd', 'conf_call', 'yes_eval', 'no_eval', 'ret'))
    back = dict(add_cmd, args=[i32(99), i32(26000004), i32(-1)])
    show = {'bank': 1, 'id': 10, 'args': [i32(1)]}
    clear = [{'bank': 1, 'id': 12, 'args': []}, {'bank': 1, 'id': 20, 'args': []}]
    items = list(flags) + ([discard] if discard else [])
    last = len(flags)
    nt = len(items)

    def tier_text(t):
        return R_DISCARD_TEXT if discard and t == last else R_TIER_TEXT + t

    def tier_desc(t):
        return R_DISCARD_DESC if discard and t == last else R_DESC_TEXT + t

    st = {0: {'entry': clear + [dict(add_cmd, args=[i32(t + 1), i32(tier_text(t)), i32(-1)]) for t in range(nt)] + [back, show],
              'conds': [{'target': 1, 'eval': MENU_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []},
          1: {'entry': [], 'exit': [], 'while': [],
              'conds': [{'target': 200 + t, 'eval': menu_pick(t + 1), 'pass': [], 'sub': []} for t in range(nt)]
              + [{'target': 9, 'eval': ALWAYS, 'pass': [], 'sub': []}]},
          9: {'entry': [], 'exit': [], 'while': [], 'conds': [ret]}}
    for t, flag in enumerate(items):
        st[200 + t] = {'entry': [dict(conf_call, args=[i32(tier_desc(t))])], 'exit': [], 'while': [],
                       'conds': [{'target': 300 + t, 'eval': yes_eval, 'pass': [], 'sub': []},
                                 {'target': 0, 'eval': no_eval, 'pass': [], 'sub': []}]}
        st[300 + t] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(1)]},
                                 {'bank': 1, 'id': 17, 'args': [i32(7), i32(R_DONE_TEXT), i32(1), i32(0), i32(1)]}],
                       'conds': [{'target': 400 + t, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
        st[400 + t] = {'entry': [], 'exit': [], 'while': [],
                       'conds': [{'target': 500 + t, 'eval': flag_is_on(flag), 'pass': [], 'sub': []},
                                 {'target': 0, 'eval': ALWAYS, 'pass': [], 'sub': []}]}
        st[500 + t] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(0)]},
                                 {'bank': 1, 'id': 17, 'args': [i32(7), i32(R_MISSING_TEXT), i32(1), i32(0), i32(1)]}],
                       'conds': [{'target': 9, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
    return st


def esd(cx, groups):
    """收进子菜单的商店数记在 cx.dark_shop"""
    cx.dark_shop = 0
    p = menu_parts(groups, cx.root)
    more, gid = p['more'], p['gid']
    hit = [(sid, c) for sid, s in more.items() for c in s['entry'] if option_of(c) in OLD]
    if not hit:
        return
    d = cx.feat.get('dark_shop') or {}
    shops = [(k, t) for k, t in SHOPS if k in d]
    # 原护符商店的调用和它调用的状态机（打开商店 → 等界面关掉 → 返回），新商店照抄
    tgt = next(x['target'] for x in more[p['disp']]['conds'] if x['eval'] == menu_pick(OLD[0]))
    call = [c for c in more[tgt]['entry'] if c['bank'] == 6]
    if len(call) != 1 or len(more[tgt]['conds']) != 1:
        raise RuntimeError('「更多选项」里原护符商店的调用状态对不上')
    done = more[tgt]['conds'][0]['eval']
    src = groups[call[0]['id'] & 0xFFFFFFFF]
    if sorted(src) != [0, 1] or [(c['bank'], c['id']) for c in src[0]['entry']] != [(1, OPEN_REGULAR)]:
        raise RuntimeError('原护符商店的状态机对不上')
    # 2026-10-31 起暗黑商店收进赐福最上面的「暗黑重铸」（sc/forge.py），「更多选项」里原来那三项全去掉
    gone = {id(c) for _, c in hit}
    for sid in {s for s, _ in hit}:
        more[sid] = dict(more[sid], entry=[c for c in more[sid]['entry'] if id(c) not in gone])
    if not shops:
        cx.notes.append('暗黑商店没加上（没有重随包的商店行），去掉了「更多选项」里原来的护符 / 魔法 / 祷告商店')
        return

    # 各商店的状态机
    sub = {}
    nid = gid
    for k, _t in shops:
        nid += 1
        while nid in groups:
            nid += 1
        dragon = k in DRAGON_SHOPS and d.get('talisman_dragon_ui', True)
        shard = k in ('sorcery', 'incant') and d.get('spell_shard_ui', True)
        cmd, menu = (OPEN_DRAGON, MENU_DRAGON) if dragon else (OPEN_SHARD, MENU_SHARD) if shard else (OPEN_REGULAR, MENU_REGULAR)
        lo, hi = d[k]
        groups[nid] = {0: dict(src[0], entry=[{'bank': 1, 'id': cmd, 'args': [i32(lo), i32(hi)]}],
                               conds=[dict(src[0]['conds'][0], eval=GRACE_WAIT(menu))]),
                       1: src[1]}
        sub[k] = nid
    entries = list(shops)
    rec = cx.feat.get('recycle') or {}
    if rec.get('flags'):
        nid += 1
        while nid in groups:
            nid += 1
        groups[nid] = recycle_group(p, rec['flags'], rec.get('discard', 0))
        register(cx, 'recycle', RECYCLE_TEXT, nid)      # 收进「暗黑重铸」（sc/forge.py），不在暗黑商店子菜单里了
    cx.recycle = bool(rec.get('flags'))
    # 子菜单（写法同双子老妪子菜单）；有上架标记的（战灰商店：打败葛瑞克；魔法 / 祷告商店：打败满月女王蕾娜菈）用 5:19 AddTalkListDataIf，标记亮了才列出来
    add_cmd = p['add_cmd']
    spell = int(d.get('spell_release') or 0)
    release = {'ash': int(d.get('ash_release') or 0), 'sorcery': spell, 'incant': spell}

    def item(j, k, t):
        if release.get(k):
            return {'bank': 5, 'id': 19, 'args': [flag_is_on(release[k]), i32(j + 1), i32(t), i32(-1)]}
        return dict(add_cmd, args=[i32(j + 1), i32(t), i32(-1)])
    st = {0: {'entry': [{'bank': 1, 'id': 12, 'args': []}, {'bank': 1, 'id': 20, 'args': []}]
                       + [item(j, k, t) for j, (k, t) in enumerate(entries)]
                       + [dict(add_cmd, args=[i32(99), i32(26000004), i32(-1)]), {'bank': 1, 'id': 10, 'args': [i32(1)]}],
              'conds': [{'target': 1, 'eval': MENU_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []},
          1: {'entry': [], 'exit': [], 'while': [],
              'conds': [{'target': 10 + j, 'eval': menu_pick(j + 1), 'pass': [], 'sub': []} for j in range(len(entries))]
              + [{'target': 9, 'eval': ALWAYS, 'pass': [], 'sub': []}]},
          9: {'entry': [], 'exit': [], 'while': [], 'conds': [p['ret']]}}
    for j, (k, _t) in enumerate(entries):
        st[10 + j] = {'entry': [{'bank': 6, 'id': sub[k], 'args': []}], 'exit': [], 'while': [],
                      'conds': [{'target': 0, 'eval': done, 'pass': [], 'sub': []}]}
    groups[gid] = st
    register(cx, 'darkshop', TEXT, gid)            # 收进「暗黑重铸」（sc/forge.py）
    cx.dark_shop = len(shops)


def texts(cx, key):
    if not getattr(cx, 'dark_shop', 0):
        return {}
    n = NAMES[key]
    d = cx.feat.get('dark_shop') or {}
    talk = {TEXT: n['menu']}
    talk.update({t: n[k] for k, t in SHOPS})
    if getattr(cx, 'recycle', False):
        rec = cx.feat.get('recycle') or {}
        talk.update(recycle_texts(key, rec.get('stones') or [1, 2, 3, 4], rec.get('discard', 0)))
    return {'EventTextForTalk': talk, 'GR_MenuText': {int(t): n[k] for k, t in (d.get('titles') or {}).items()}}


def recycle_texts(key, stones, discard=0):
    zh = key == 'zh'
    out = {RECYCLE_TEXT: '武器回收' if zh else 'Recycle Weapons',
           R_DONE_TEXT: '已回收，武器重铸石稍后放进背包' if zh else 'Recycled. Weapon Reforge Stones will arrive in your inventory shortly.',
           R_MISSING_TEXT: ('现在无法回收：暗黑重铸的组件没有加载，请用启动器重新启动游戏。' if zh
                            else "Can't recycle right now: a Darkforge component didn't load. Please restart the game from the launcher.")}
    for t, (tz, te, desc) in enumerate(TIERS):
        out[R_TIER_TEXT + t] = tz if zh else te
        if zh:
            out[R_DESC_TEXT + t] = ('回收背包里所有%s的新武器，换成武器重铸石\n（每把：普通 %d 个、稀有 %d 个、史诗 %d 个、传说 %d 个；\n'
                                    '装备着的、锁定的、仓库里的不回收，回收后不能买回）。\n%s\n\n确定回收吗？'
                                    % ((desc,) + tuple(stones) + (field(R_KIND0 + t, 'zh'),)))
        else:
            out[R_DESC_TEXT + t] = ('Recycle all %s new armaments in your inventory into Weapon Reforge Stones\n'
                                    '(per weapon: Common %d, Rare %d, Epic %d, Legendary %d; equipped, locked and stored ones are kept;\n'
                                    'recycled weapons cannot be bought back).\n%s\n\nRecycle them?'
                                    % ((TIERS_EN[t],) + tuple(stones) + (field(R_KIND0 + t, 'en'),)))
    if discard:
        out[R_DISCARD_TEXT] = '回收全部弃置' if zh else 'Recycle all discarded'
        if zh:
            out[R_DISCARD_DESC] = ('回收背包里所有打了「弃置」标记的武器，换成武器重铸石\n'
                                   '（每把按自己的稀有度：普通 %d 个、稀有 %d 个、史诗 %d 个、传说 %d 个；\n'
                                   '装备着的、锁定的、仓库里的不回收，回收后不能买回）。\n%s\n\n确定回收吗？'
                                   % (tuple(stones) + (field(R_DISCARD_KIND, 'zh'),)))
        else:
            out[R_DISCARD_DESC] = ('Recycle all armaments marked as discarded in your inventory into Weapon Reforge Stones\n'
                                   '(each by its own rarity: Common %d, Rare %d, Epic %d, Legendary %d; equipped, locked and stored ones are kept;\n'
                                   'recycled weapons cannot be bought back).\n%s\n\nRecycle them?'
                                   % (tuple(stones) + (field(R_DISCARD_KIND, 'en'),)))
    return out
