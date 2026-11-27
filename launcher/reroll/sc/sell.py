# -*- coding: utf-8 -*-
"""一键出售（settings.json 的 quick_sell；真正卖东西的是 dll\\dark_sell\\dark_sell.dll）：赐福「暗黑重铸」子菜单里的「一键出售」（sc/forge.py；以前在赐福主菜单），
两级菜单：先选类别（背包的武器 / 防具 / 护符、木箱里的武器 / 防具 / 护符），再选档位（普通 / 稀有及以下 / 史诗及以下 / 传说及以下）。选中先弹确认框，
选「是」就点亮请求标记 FLAGS[类别 × 4 + 档位]，弹一句「已出售」；DLL 每帧看这些标记，看到就把背包里没装备着的（木箱那三类是木箱里的）、
没锁定的、这一类这一档及以下的随机装备按游戏里的售价卖掉、熄灭标记。
用户 2026-11-25：「增加一个一键卖出木箱里装备的功能 也分为三类」——木箱（赐福「整理木箱」）的三类接在背包三类后面（类别 3~5）。
提示框关掉时标记还亮着 = DLL 没加载：熄灭标记（免得以后装上 DLL 时突然卖掉），改弹「没有加载出售 DLL」。
请求标记在官方定义里「予備」的 93000 块（背包，93000~93011）和 92000 块（木箱，92780~92791）（会存档、谁都没用；以前的位置被写坏过、和事件编号撞过，
见 dev\\event_flags\\说明.md）。
确认框里的预计件数 / 卢恩：固定长度的占位「〔#S<序号，0~9 / A~N 一位> …〕」（英文用 [ ]），dark_sell 坐在赐福点时每半秒按背包、木箱现状算好
「预计卖出 N 件，约 R 卢恩」交给 dark_core 的取文字钩子（msghook.rs），游戏取这段文字时换掉占位；没装 DLL 就显示占位里的提示"""
from .esdutil import ALWAYS, DIALOG_WAIT, MENU_WAIT, flag_is_on, register, i32, menu_parts, menu_pick

OPTION, TEXT, CAT_TEXT, DONE_TEXT, MISSING_TEXT = 34, 80090110, 80090111, 80090115, 80090116
TIER_TEXT, DESC_TEXT = 80090117, 80090121          # 档位名 4 个、背包的确认框 12 个（类别 × 4 + 档位）
CHEST_CAT_TEXT, CHEST_DESC_TEXT = 80090500, 80090510   # 木箱的类别 3 个、确认框 12 个（80090500 段没人用，2026-11-25）
# 「出售全部弃置」（用户 2026-11-26）：类别列表的第 7 项，选了直接进确认框（没有档位），标记单独一个 93019
# （93019 原为圆顶请求、已停用，dev\event_flags\说明.md 明确可再分配；最初用 93012 和许愿石撞了）
DISCARD_CAT_TEXT, DISCARD_DESC_TEXT, DISCARD_FLAG = 80090130, 80090131, 93019
# 和 dark_sell 的 SELL_FLAGS 一致：背包 3 类 × 4 档，再木箱 3 类 × 4 档
FLAGS = tuple(range(93000, 93012)) + tuple(range(92780, 92792))
KINDS = (('武器', 'weapons'), ('防具', 'armor'), ('护符', 'talismans'))
CATS = tuple((zh, en, False) for zh, en in KINDS) + tuple((zh, en, True) for zh, en in KINDS)    # (名字, 英文, 是不是木箱)
TIERS = (('普通', 'Common', '「普通」'),
         ('稀有及以下', 'Rare and below', '「稀有」及以下（普通、稀有）'),
         ('史诗及以下', 'Epic and below', '「史诗」及以下（普通、稀有、史诗）'),
         ('传说及以下', 'Legendary and below', '「传说」及以下（全部稀有度）'))
TIERS_EN = ('Common', 'Common and Rare', 'Common, Rare and Epic', 'Common, Rare, Epic and Legendary')
FIELD_LEN = {'zh': 30, 'en': 44}                   # 含两端括号的字符数（DLL 按括号找范围，长度可以不同）
DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ'     # 占位序号（dark_core msghook.rs 的 marker_at 认 0~9、A~Z）
assert len(FLAGS) == len(CATS) * len(TIERS) <= len(DIGITS)


def on(cx):
    return bool(cx.S.get('quick_sell', True))


def cat_text(c):
    return CAT_TEXT + c if c < len(KINDS) else CHEST_CAT_TEXT + c - len(KINDS)


def desc_text(i):
    n = len(KINDS) * len(TIERS)
    return DESC_TEXT + i if i < n else CHEST_DESC_TEXT + i - n


def field(i, lang):
    if lang == 'zh':
        body = '#S%s 正在估算……' % DIGITS[i]
        return '〔' + body + ' ' * (FIELD_LEN['zh'] - 2 - len(body)) + '〕'
    body = '#S%s estimating...' % DIGITS[i]
    return '[' + body + ' ' * (FIELD_LEN['en'] - 2 - len(body)) + ']'


def esd(cx, groups):
    if not on(cx):
        return
    p = menu_parts(groups, cx.root)
    add_cmd, conf_call, yes_eval, no_eval, ret, gid = (p[k] for k in ('add_cmd', 'conf_call', 'yes_eval', 'no_eval', 'ret', 'gid'))
    nc, nt = len(CATS), len(TIERS)
    ncat = nc + 1                      # 第 7 项「出售全部弃置」（没档位，直接确认框）
    disc_i = nc * nt                   # 它在确认框 / 标记状态里的序号（24）
    cat_label = lambda c: DISCARD_CAT_TEXT if c == nc else cat_text(c)
    back = dict(add_cmd, args=[i32(99), i32(26000004), i32(-1)])
    show = {'bank': 1, 'id': 10, 'args': [i32(1)]}
    clear = [{'bank': 1, 'id': 12, 'args': []}, {'bank': 1, 'id': 20, 'args': []}]
    # 0 / 1：类别列表；100 + c / 110 + c：这一类的档位列表；200 + i：确认框；300 + i：点亮标记、弹提示；400 + i：看 DLL 有没有处理；500 + i：没处理
    st = {0: {'entry': clear + [dict(add_cmd, args=[i32(c + 1), i32(cat_label(c)), i32(-1)]) for c in range(ncat)] + [back, show],
              'conds': [{'target': 1, 'eval': MENU_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []},
          1: {'entry': [], 'exit': [], 'while': [],
              'conds': [{'target': 100 + c, 'eval': menu_pick(c + 1), 'pass': [], 'sub': []} for c in range(nc)]
              + [{'target': 200 + disc_i, 'eval': menu_pick(ncat), 'pass': [], 'sub': []}]
              + [{'target': 9, 'eval': ALWAYS, 'pass': [], 'sub': []}]},
          9: {'entry': [], 'exit': [], 'while': [], 'conds': [ret]}}
    for c in range(nc):
        st[100 + c] = {'entry': clear + [dict(add_cmd, args=[i32(t + 1), i32(TIER_TEXT + t), i32(-1)]) for t in range(nt)] + [back, show],
                       'conds': [{'target': 110 + c, 'eval': MENU_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
        st[110 + c] = {'entry': [], 'exit': [], 'while': [],
                       'conds': [{'target': 200 + c * nt + t, 'eval': menu_pick(t + 1), 'pass': [], 'sub': []} for t in range(nt)]
                       + [{'target': 0, 'eval': ALWAYS, 'pass': [], 'sub': []}]}
        for t in range(nt):
            i = c * nt + t
            flag = FLAGS[i]
            st[200 + i] = {'entry': [dict(conf_call, args=[i32(desc_text(i))])], 'exit': [], 'while': [],
                           'conds': [{'target': 300 + i, 'eval': yes_eval, 'pass': [], 'sub': []},
                                     {'target': 100 + c, 'eval': no_eval, 'pass': [], 'sub': []}]}
            st[300 + i] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(1)]},
                                     {'bank': 1, 'id': 17, 'args': [i32(7), i32(DONE_TEXT), i32(1), i32(0), i32(1)]}],
                           'conds': [{'target': 400 + i, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
            st[400 + i] = {'entry': [], 'exit': [], 'while': [],
                           'conds': [{'target': 500 + i, 'eval': flag_is_on(flag), 'pass': [], 'sub': []},
                                     {'target': 100 + c, 'eval': ALWAYS, 'pass': [], 'sub': []}]}
            st[500 + i] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(0)]},
                                     {'bank': 1, 'id': 17, 'args': [i32(7), i32(MISSING_TEXT), i32(1), i32(0), i32(1)]}],
                           'conds': [{'target': 9, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
    # 第 7 项「出售全部弃置」：确认框 → 点亮 DISCARD_FLAG → 看 DLL 处没处理（dark_sell 的 SELL_DISCARD_FLAG）
    i, flag = disc_i, DISCARD_FLAG
    st[200 + i] = {'entry': [dict(conf_call, args=[i32(DISCARD_DESC_TEXT)])], 'exit': [], 'while': [],
                   'conds': [{'target': 300 + i, 'eval': yes_eval, 'pass': [], 'sub': []},
                             {'target': 0, 'eval': no_eval, 'pass': [], 'sub': []}]}
    st[300 + i] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(1)]},
                             {'bank': 1, 'id': 17, 'args': [i32(7), i32(DONE_TEXT), i32(1), i32(0), i32(1)]}],
                   'conds': [{'target': 400 + i, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
    st[400 + i] = {'entry': [], 'exit': [], 'while': [],
                   'conds': [{'target': 500 + i, 'eval': flag_is_on(flag), 'pass': [], 'sub': []},
                             {'target': 0, 'eval': ALWAYS, 'pass': [], 'sub': []}]}
    st[500 + i] = {'entry': [{'bank': 1, 'id': 11, 'args': [i32(flag), i32(0)]},
                             {'bank': 1, 'id': 17, 'args': [i32(7), i32(MISSING_TEXT), i32(1), i32(0), i32(1)]}],
                   'conds': [{'target': 9, 'eval': DIALOG_WAIT, 'pass': [], 'sub': []}], 'exit': [], 'while': []}
    groups[gid] = st
    register(cx, 'sell', TEXT, gid)          # 收进赐福最上面的「暗黑重铸」（sc/forge.py）


def texts(cx, key):
    if not on(cx):
        return {}
    if key == 'zh':
        out = {TEXT: '一键出售', DONE_TEXT: '已出售，卢恩已到账',
               MISSING_TEXT: '现在无法出售：暗黑重铸的组件没有加载，请用启动器重新启动游戏。'}
    else:
        out = {TEXT: 'Quick sell', DONE_TEXT: 'Sold. Runes received.',
               MISSING_TEXT: "Can't sell right now: a Darkforge component didn't load. Please restart the game from the launcher."}
    for c, (zh, en, chest) in enumerate(CATS):
        if key == 'zh':
            out[cat_text(c)] = ('出售木箱里的' if chest else '出售') + zh
        else:
            out[cat_text(c)] = 'Sell ' + en + (' in chest' if chest else '')
    for t, (zh, en, desc) in enumerate(TIERS):
        out[TIER_TEXT + t] = zh if key == 'zh' else en
        for c, (czh, cen, chest) in enumerate(CATS):
            i = c * len(TIERS) + t
            if key == 'zh' and chest:
                text = '出售木箱里所有%s的%s，按售价换成卢恩\n（锁定的不卖）。\n%s\n\n确定出售吗？' % (desc, czh, field(i, 'zh'))
            elif key == 'zh':
                text = ('出售背包里所有%s的%s，按售价换成卢恩\n（装备着的、锁定的、木箱里的不卖）。\n%s\n\n确定出售吗？'
                        % (desc, czh, field(i, 'zh')))
            elif chest:
                text = ('Sell all %s %s in the chest for runes\n(locked items are kept).\n%s\n\nSell them?'
                        % (TIERS_EN[t], cen, field(i, 'en')))
            else:
                text = ('Sell all %s %s in your inventory for runes\n(equipped, locked and chest items are kept).\n%s\n\nSell them?'
                        % (TIERS_EN[t], cen, field(i, 'en')))
            out[desc_text(i)] = text
    # 第 7 项「出售全部弃置」：类别名 + 确认框（占位序号 24 = #SO，dark_sell 的 write_fields 第 25 项）
    out[DISCARD_CAT_TEXT] = '出售全部弃置' if key == 'zh' else 'Sell all discarded'
    if key == 'zh':
        out[DISCARD_DESC_TEXT] = ('出售背包里所有打了「弃置」标记的装备，按售价换成卢恩\n'
                                  '（装备着的、锁定的、木箱里的不卖）。\n%s\n\n确定出售吗？'
                                  % field(len(CATS) * len(TIERS), 'zh'))
    else:
        out[DISCARD_DESC_TEXT] = ('Sell all gear marked as discarded in your inventory for runes\n'
                                  '(equipped, locked and chest items are kept).\n%s\n\nSell them?'
                                  % field(len(CATS) * len(TIERS), 'en'))
    return {'EventTextForTalk': out}
