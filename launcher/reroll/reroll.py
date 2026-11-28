# -*- coding: utf-8 -*-
"""Dark Reforged 重随：每次启动前在原版（或随机器输出）的 regulation 上做改动，写到重随包目录（out\\reroll，YAFSML 在随机器输出之后加载）。

各功能在 rr/ 下按模块分开（见 rr/__init__.py），这里只负责：读参数和设置 → 按顺序调用各步骤 → 写回、校验、出报告。
规则和开关在 settings.json。给游戏脚本覆盖包（scripts.py）看的「这次建了什么」写成重随包里的 features.json。

用法（由启动器调用）：
  python reroll.py --base-reg <regulation.bin> --msg-src <含 engus / zhocn 的文字包目录> [--msg-src <备用>] --out <重随包目录>
                   --dll-dir <含 oo2core / libzstd 的目录> [--dll-dir ...] [--seed N]
                   [--randomizer-dir <随机器目录> --maps-out <修补地图的覆盖包目录>]（开了敌人随机时）
"""
import argparse
import glob
import json
import os
import random
import re
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import erlib  # noqa: E402
from rr import affixes, ailments, aoe, arena, ashcont, ashes, ashtest, atkfwd, bossaward, bosses, bossmods, containers, curses, darkshop, difficulty, dummy, enemies, extragrace, festive, foefx, hpunify, inherit, manuals, mariner, nightmare, notes, overlimit, plague, procs, unseen, quiver, reforge, remembrance, reuse, serpent, shops, skillgems, sounds, startgear, tweaks, unidentified, upgrade, wish, wishstone, world  # noqa: E402
from rr.common import Ctx, log  # noqa: E402
import progress  # noqa: E402

LANGS = ('engus', 'zhocn')
SELL_DIR = os.path.join(HERE, '..', '..', 'dll', 'dark_sell')
CORE_DIR = os.path.join(HERE, '..', '..', 'dll', 'dark_core')
# 报告里出现这些词的行才打到启动日志里（「0 个…跳过」这种没出问题的不算）
PROBLEM_WORDS = ('没加上', '没找到', '找不到', '跳过', '失效', '失败', '不足', '不够', '冲突', '超过', '错误')


def parse_args():
    ap = argparse.ArgumentParser()
    ap.add_argument('--base-reg', required=True)
    ap.add_argument('--msg-src', action='append', required=True,
                    help='文字包来源目录（下面是 engus / zhocn 等语言目录）；可以给几个，每种语言用第一个有的')
    ap.add_argument('--out', required=True)
    ap.add_argument('--dll-dir', action='append', default=[])
    ap.add_argument('--settings', default=os.path.join(HERE, 'settings.json'))
    ap.add_argument('--seed', type=int, default=None)
    ap.add_argument('--label', default='')
    ap.add_argument('--noop', action='store_true', help='排查用：什么都不改，只把基础 regulation 重新打包写出')
    ap.add_argument('--state', default=os.path.join(os.environ.get('APPDATA', HERE), 'EldenRing', 'DarkReforged', '角色记录', 'once.json'),
                    help='一次性重随的记录（测试时可指向别处）')
    ap.add_argument('--randomizer-dir', default='', help='有敌人随机时：随机器目录（读它输出的地图和日志）')
    ap.add_argument('--maps-out', default='', help='有敌人随机时：修补后地图的覆盖包目录')
    ap.add_argument('--boss-mods', default='', help='这次加载的 Boss 模组（键,键；启动器按角色难度定，见 rr/bossmods.py）；不给 = 不加载')
    ap.add_argument('--boss-arena', default='', help='敌人随机的高难度世界（这些 Boss 留在原位）：要把场地移植上去的键')
    return ap.parse_args()


def rand_options(rand_dir):
    """随机器最近一次成功的随机化用的选项（spoiler 日志第一行「Options and seed:」，失败的日志里有报错，跳过）；没有返回空集合"""
    if not rand_dir:
        return set()
    for lp in sorted(glob.glob(os.path.join(rand_dir, 'spoiler_logs', '*_log_*.txt')), key=os.path.getmtime, reverse=True):
        t = open(lp, encoding='utf-8', errors='replace').read()
        if 'Exception' in t or not t.startswith('Options and seed:'):
            continue
        return set(t.split('\n', 1)[0].split(':', 1)[1].split())
    return set()


def load(a):
    """读设置和 regulation，建好上下文；文字包从来源复制一份到重随包里（各步骤在这份上改）"""
    erlib.DLL_DIRS = [d for d in a.dll_dir if d]
    S = json.load(open(a.settings, encoding='utf-8'))
    seed = a.seed if a.seed is not None else int.from_bytes(os.urandom(4), 'little')
    os.makedirs(a.out, exist_ok=True)
    # 并行进程数（parallel_jobs：0 = 按这一刻的空闲核数和可用内存自动定，1 = 不并行）
    erlib.JOBS, why = erlib.auto_jobs(S.get('parallel_jobs', 0))
    log('  并行：%d 个进程（%s）' % (erlib.JOBS, why))
    log('  [1/4] 解开 regulation...')
    bnd = erlib.Bnd4(erlib.read_regulation(a.base_reg))
    P_en, P_mp = erlib.Param(bnd.get('ItemLotParam_enemy.param')), erlib.Param(bnd.get('ItemLotParam_map.param'))
    for nm, P in (('ItemLotParam_enemy', P_en), ('ItemLotParam_map', P_mp)):
        if P.row_size != 152:
            raise RuntimeError('%s 行大小 %d 与预期 152 不符（游戏版本不对？）' % (nm, P.row_size))
    msg_out = os.path.join(a.out, 'msg')
    shutil.rmtree(msg_out, ignore_errors=True)
    for lang in LANGS:
        src = next((p for p in (os.path.join(d, lang, 'item_dlc02.msgbnd.dcx') for d in a.msg_src) if os.path.isfile(p)), None)
        if src:
            os.makedirs(os.path.join(msg_out, lang))
            shutil.copy2(src, os.path.join(msg_out, lang, 'item_dlc02.msgbnd.dcx'))
    data_dir = os.path.join(HERE, 'data')
    # 自制特效包（dev\item_system\make_sfx.py 做的 sfxbnd_c0000，原版没有这个文件）：整个拷进输出，跟着 DR_Reroll 挂载
    sfx_out = os.path.join(a.out, 'sfx')
    shutil.rmtree(sfx_out, ignore_errors=True)
    if os.path.isdir(os.path.join(data_dir, 'sfx')):
        shutil.copytree(os.path.join(data_dir, 'sfx'), sfx_out)
    # 改了判定号的动作包（专属战技战灰）：先删掉，这次重随写好 regulation 后再放（rr/skillgems.py place_anim）
    shutil.rmtree(skillgems.chr_out(a), ignore_errors=True)
    # 随机器开了物品随机（选项里有 item）：它已经打乱了商店、骨灰，商店重抽、骨灰打乱不再做（dev\item_rand\物品随机兼容.md）
    item_rand = 'item' in rand_options(a.randomizer_dir)
    if item_rand:
        log('  随机器开着物品随机：商店重抽、骨灰打乱这次跳过')
    return Ctx(a=a, S=S, seed=seed, rng=random.Random(seed), data_dir=data_dir, bnd=bnd, P_en=P_en, P_mp=P_mp, msg_out=msg_out,
               map_tiers=json.load(open(os.path.join(data_dir, 'map_tiers.json'), encoding='utf-8')), item_rand=item_rand)


def run_steps(ctx):
    """各步骤的先后顺序有讲究（都用同一个随机数，顺序变了结果就变；往地图掉落表加固定编号的要在保底锻造石之前）"""
    log('  [2/4] 改参数...')
    bossmods.build(ctx)                       # Boss 模组（只在启动器决定加载时）：模组改的参数行先写进去，后面所有步骤叠在它上面
    ailments.resist(ctx)                      # 发狂 / 咒死：原来免疫的敌人改成照冻伤等抗性（要在复制敌人参数之前）
    unidentified.prepare(ctx)                 # 未鉴定物品的道具、敌人掉落的复制表（enemies.hp 里用）
    enemies.hp(ctx)                           # 敌人分档、血量按地区、Boss 战随从不掉东西、多掉未鉴定物品（敌人随机时按位置修正，stage3）
    enemies.scarab_hp(ctx)                    # 粪金龟实际血量统一
    ashes.shuffle(ctx)                        # 骨灰获取处一次性打乱
    world.start_spirit_bell(ctx)              # 开局送召魂铃（雷娜那里只给骨灰）
    containers.build(ctx)                     # 新装备的容器 + 发货表
    upgrade.build(ctx)                        # 生成武器的强化：最高 +5、每级 1 块石头、按稀有度成长（DLL 改写容器行时换上）
    # 下面往 Boss 奖励表后面接东西：要在上面几个固定编号的掉落表之后，不占它们的编号；传说优先占奖励表的空号
    unidentified.boss_drops(ctx)              # Boss 击杀奖励多给未鉴定的传说
    unidentified.world_drops(ctx)             # 宝箱、黄金卢恩光点、人形 NPC 多给未鉴定物品
    unidentified.no_vanilla_gear(ctx)         # 宝箱、支线奖励里的原版装备换成未鉴定物品；白面具给勋章时多给 4 件传说
    bosses.gifts(ctx)                         # Boss 龙心脏 + 星光碎片（融锻之心由 dark_core 发：bosses.fusion_lines → fusion.txt）
    bosses.smithing_stones(ctx)               # Boss 保底锻造石
    bossaward.build(ctx)                      # 上面几步放不下奖励表的：放进单独的表，击败标记亮起时事件发放（sc/bossaward.py）
    bosses.no_weapons(ctx)                    # 掉落里的原版装备删掉（要在往掉落表加东西的步骤之后）
    world.fast_travel_anywhere(ctx)           # 地下墓地等原版禁止传送的地方也能传送
    world.spirit_summon_anywhere(ctx)         # 骨灰召唤范围放宽（照法魂改每个石碑：范围 9999 米、不限区域、能再次召唤）
    extragrace.build(ctx)                     # 额外的赐福（魔法学院大书库门前）：传送行 + 地名（地图部件、登记事件在 sc/extragrace.py）
    world.no_fall_damage(ctx)                 # 不受坠落伤害 + 不会摔死的效果（玩家脚本每帧挂上，见 sc/hks.py）
    tweaks.official_fixes(ctx)                # 官方新版 regulation 的修正（底子是 1.17.1；1.17.3 只差兽骨诱惑箭残留区域的攻击行）
    tweaks.twin_rune_arc(ctx)                 # 双子老妪不限量卖卢恩弯弧
    tweaks.skill_flat_damage(ctx)             # 战技的固定伤害 × skill_flat_damage_mult
    tweaks.split_element_moves(ctx)           # 只吃一种元素的普通攻击 / 专属战技判定（蕾拉娜的对剑、夜与火之剑光束等）：动作值按原武器的原版面板统一（在范围放大、专属战技战灰之前）
    tweaks.skill_mv_scale(ctx)                # 单个战技的动作值 × 倍数（发劲、多重投掷 ×0.5）
    tweaks.skill_correct_follow_weapon(ctx)   # 战技的能力值补正跟武器（去掉战技自己的属性补正表）
    aoe.aoe_radius(ctx)                       # 范围战灰的判定半径等比放大到 8 米、子弹特效换放大的副本（复制的行要带上面改过的伤害；在词条库之前）
    tweaks.item_damage(ctx)                   # 道具伤害统一：壶 / 调香 / 飞刀 / 大壶 / 石头等道具的攻击都加子类别 108（词条「道具伤害」只认它）
    tweaks.item_scaling(ctx)                  # 道具补正：道具的虚拟武器的补正值 × item_scaling_mult
    tweaks.catalyst_school(ctx)               # 法杖 / 圣印记的流派加成统一（词条库里的 pct，2026-11-13 起 25%）
    tweaks.lightning_ram(ctx)                 # 雷电羊球按住一直滚：只有第一下消耗专注值
    tweaks.memory_of_grace(ctx)               # 赐福记忆传回去不丢卢恩
    tweaks.lantern_light(ctx)                 # 提灯照得更亮（光源换成 sfxbnd_c0000 里只改了衰减的 990060）
    tweaks.lantern_persist(ctx)               # 提灯传送后不熄（效果 3245 saveCategory → 0；以前靠 ErdTools 的 persistent_lantern，它在加载时扫参数仓库会崩）
    tweaks.shard_cap(ctx)                     # 星光碎片随身上限 10 → starlight_shard_max（默认 99）
    tweaks.scaling_numbers(ctx)               # 武器补正显示数值：删掉补正 → 字母的那组表（数字由 dll\dark_core 印）
    difficulty.build(ctx)                     # 局内难度：每档一个玩家特殊效果（赐福菜单里选，见 sc/difficulty.py）
    hpunify.build(ctx)                        # 高难度血量补正：死亡 / 不可能里敌人的最大血量倍率（效果行 + 每个位置的倍率，dll\dark_core 挂）
    bossmods.after_hpunify(ctx)               # Boss 模组场地新加的随从跟着 Boss 乘同一档补正
    nightmare.build(ctx)                      # 魇化精英与魇印：精英的强化 / 描边效果行（挑精英、魇火、魇印在 dll\dark_core）
    arena.build(ctx)                          # 斗技场试炼：每个竞技场的 Boss / 精英池、敌人参数副本（地图、事件、雕像对话在 sc/arena.py，执行在 dll\dark_core；要在词条库之前：千钧认 Boss 看敌人参数）
    foefx.build(ctx)                          # 敌人效果包（通用）：自定义效果行（斗技场按波次挂，挂在 dll\dark_core 的 foefx.rs；在斗技场之后：它的规则里直接写的效果也在这里做）
    festive.build(ctx)                        # 庆典油脂：卢恩越多伤害越高、每秒扣卢恩（扣卢恩、换档在 dll\dark_core）
    mariner.build(ctx)                        # 战灰「唤声船」：所有武器能装（含法杖 / 圣印记），骑船、瞬移、原版唤声船位置的 Boss 发碎片在 dll\dark_core（要在词条库之前）
    skillgems.build(ctx)                      # 专属战技战灰：判定行复制到共享层 + 战灰 + 文字，动作包改判定号（在范围放大之后，暗黑商店之前）
    skillgems.any_weapon(ctx)                 # 熔炉百相之羽、唤声船、雷电羊球能装在任意武器上
    dummy.build(ctx)                          # 训练木桩的敌人参数（放进地图在 sc/training.py）
    affixes.build(ctx)                        # 词条库要用、原版里没有的参数行（data\affixes\rows.json）
    curses.build(ctx)                         # 咒痕：诅咒 / 祝福的效果行、烙印的准星（要在词条库之后：照猎印准星的光效行复制）
    notes.build(ctx)                          # 原版闪烁提示：魇火、魇印、咒痕碎片的文字和编号（在魇化、咒痕之后；事件在 sc/notes.py）
    sounds.build(ctx)                         # 事件脚本放的音效（相杀挡下攻击的那一声 + 试听候选；事件在 sc/sounds.py，标记写进 clash.txt）
    quiver.build(ctx)                         # 箭袋护符（已停用）：只留参数和文字，老存档里的不会变成参数不存在的物品
    plague.build(ctx)                         # 词条疫病的敌人一侧：抗性倍率效果行（词条本身在词条库）
    darkshop.build(ctx)                       # 暗黑商店的护符 / 武器 / 道具商店行（护符模板要在词条库之后）
    startgear.build(ctx)                      # 开局遗物（护符模板）和开局装备（DLL 按种子生成，一次性重随）
    unidentified.quest_gear_finish(ctx)       # 支线换成的同本体史诗装备接进开局装备的 start 行（startgear 会清空 start_items）
    remembrance.build(ctx)                    # 追忆换取：接肢 → 飞行战灰，其他武器格 → 成品传说（DLL 按种子生成，一次性重随）
    serpent.build(ctx)                        # 大蛇狩猎矛换成 DR 版本（DLL 在人物拿到时按属性生成）
    manuals.build(ctx)                        # 开局说明书（进游戏时缺哪本补哪本，见 sc/start.py）
    wish.build(ctx)                           # 向星辰许愿：直接抽成品（数据写进 items.txt，菜单 sc/wish.py，生成 dll\dark_core）
    reforge.build(ctx)                        # 武器重铸石 + 暗黑商店「装备回收」（重抽、回收在 dll\dark_core，菜单 sc/darkshop.py）
    ashcont.build(ctx)                        # 骨灰稀有度 + 词条：空容器和定义文件（拿到骨灰时 dll\dark_core 换成容器）
    ashtest.build(ctx)                        # 骨灰词条的测试骨灰（F9「骨灰词条测试」发放，提案 dev\item_system\骨灰词条提案.md）
    overlimit.build(ctx)                      # 超限锻造石：武器 / 防具两种道具（装备工坊「超限」页用，执行在 dll\dark_core；掉落另做）
    inherit.build(ctx)                        # 赐福「装备工坊」：标记 + 道具融锻之心（Boss 掉落：dark_core 的 fusion 模块，定义 bosses.fusion_lines；界面 dll\dark_ui，执行 dll\dark_core，菜单 sc/inherit.py）
    wishstone.build(ctx)                      # 许愿石：死亡以上 Boss 掉（和融锻之心一起由 dark_core 的 fusion 模块发），赐福「许愿石升级」直接升一级（sc/wishstone.py、dark_core 的 wishstone 模块）
    shops.build(ctx)                          # 商人商店重抽、甲虫掉战灰（一次性重随）、商店价格统一（要在追忆换取之后）
    ailments.effects(ctx)                     # 发狂发作扣 5% 生命、咒死发作改成降抗性（要在所有加带异常的效果行之后）
    atkfwd.build(ctx)                         # 被命中标记顶掉的油脂、附魔祷告、蛇弓：dark_core 补挂用的副本（要在所有改效果行的步骤之后）


DEF_PATHS = []     # 这次写 / 删的定义文件（记进沿用记录，rr/reuse.py）


def write_def(path, head, lines):
    """DLL 旁边的定义文件：lines 为空就删掉（DLL 看不到这个文件就什么都不做）"""
    if not os.path.isdir(os.path.dirname(path)):
        return
    DEF_PATHS.append(path)
    if not lines:
        if os.path.exists(path):
            os.remove(path)
        return
    tmp = path + '.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='\n') as f:
        f.write('# ' + head + '。每次重随自动生成，不要手改\n')
        f.writelines(l + '\n' for l in lines)
    os.replace(tmp, path)


def write_dll_defs(ctx):
    S = ctx.S
    # 局内难度的档位（dark_sell 写日志用）：一行「档数<TAB>普通是第几档（0 起）<TAB>各档名字…」
    _, ts = difficulty.tiers(S)
    write_def(os.path.join(SELL_DIR, 'difficulty_tiers.txt'), '局内难度的档位：档数、普通是第几档（0 起）、各档名字',
              ['\t'.join([str(len(ts)), str(difficulty.normal_index(ts))] + [t['name'] for t in ts])] if ctx.features.get('difficulty') and ts else [])
    # 庆典油脂（dark_core 每秒扣卢恩、按卢恩换加伤档位用）：油脂效果编号、档位起点（+k% = 起点 + k）、档数、满加成的卢恩、每秒扣的百分比
    c = festive.cfg(S)
    write_def(os.path.join(CORE_DIR, 'festive.txt'), '庆典油脂：油脂效果编号、档位起点（+k% = 起点 + k）、档数、满加成的卢恩、每秒扣的百分比',
              ['%s\t%d\t%d\t%d\t%g' % (','.join(map(str, festive.GREASE_SP)), festive.BASE_SP, c['max_pct'], c['full'], c['drain'])] if c else [])
    # 战灰「唤声船」（dark_core 的 boat 模块：骑船、瞬移、原版唤声船位置的 Boss 发碎片 / 战灰）：编号、蓝耗、标记
    write_def(os.path.join(CORE_DIR, 'boat.txt'), '战灰「唤声船」：战灰、碎片、请求 / 船 / 瞬移效果、蓝耗、三处 Boss 的击败标记、标记', mariner.def_lines(S))
    # 武器补正显示具体数值（dark_core 启动时改代码，有这个文件才改）
    write_def(os.path.join(CORE_DIR, 'scaling.txt'), '武器补正显示具体数值（settings 的 weapon_scaling_numbers）',
              ['1'] if S.get('weapon_scaling_numbers', True) else [])
    # 新装备系统：容器、未鉴定道具、发货表（dark_core 鉴定、改写、发货用）
    containers.write_def(ctx, os.path.join(CORE_DIR, 'items.txt'))
    affixes.write_def(ctx, os.path.join(CORE_DIR, 'affixes.txt'))   # 同目录还写 runtime.txt
    # 相杀（dark_core 的 clash 模块：所有相杀效果行固定受到 40% 伤害，即减伤 60%）
    write_def(os.path.join(CORE_DIR, 'clash.txt'), '相杀（rr/affixes.py clash_lines）：row 效果行 l|r（左 / 右手武器） 固定受到伤害倍率 0.4（减伤 60%）',
              affixes.clash_lines(ctx))
    # 玩家普通攻击动作的攻击判定时间（积木引擎 while 的 hit：相杀按判定开窗口）
    write_def(os.path.join(CORE_DIR, 'hitwin.txt'), '玩家普通攻击动作的攻击判定时间（rr/affixes.py hitwin_lines）：hitwin 动作编号 判定开始 判定结束（秒）',
              affixes.hitwin_lines(ctx))
    # 敌人攻击动作的攻击判定时间段（积木引擎的见切：翻滚无敌时敌人的判定正好在才算）
    write_def(os.path.join(CORE_DIR, 'foewin.txt'), '敌人攻击动作的攻击判定时间段（rr/affixes.py foewin_lines）：foewin 模型号 动作编号 开始,结束[;开始,结束…]（秒）',
              affixes.foewin_lines(ctx))
    DEF_PATHS.extend(os.path.join(CORE_DIR, f) for f in ('items.txt', 'affixes.txt', 'runtime.txt'))
    # 触发词条的属性补正（dark_core 的 procs 模块按人物属性改这些攻击行 / 治疗效果；法球、引爆的倍率也照 scale 行）
    write_def(os.path.join(CORE_DIR, 'procs.txt'), '触发词条的属性补正（rr/procs.py）：scale 补正曲线 属性 倍率；atk 攻击行 物理 魔力 火 雷 圣；heal 效果 changeHpPoint',
              procs.def_lines(ctx) if getattr(ctx, 'containers', None) else [])
    # 高难度血量补正（dark_core 的 hpunify 模块：这几档的当前档标记亮着时给在场的敌人挂最大血量倍率）
    write_def(os.path.join(CORE_DIR, 'hpunify.txt'), '高难度血量补正（rr/hpunify.py）：flag 当前档标记；range 效果起点 行数；e 实体 效果；n 敌人参数 效果',
              hpunify.def_lines(ctx))
    # 骨灰稀有度（dark_core 的 gear\ashes.rs：原版骨灰进背包就换成容器）
    write_def(os.path.join(CORE_DIR, 'ashes.txt'), '骨灰容器（rr/ashcont.py）：cont 容器；rar 稀有度；aff 数值词条；bonus / rally；lvl 等级后缀；kind 原版骨灰',
              ashcont.def_lines(ctx))
    # 超限锻造石掉落（dark_core 的 ovldrop 模块：死亡 / 不可能里 Boss 每档第一次、附近小怪按几率）
    write_def(os.path.join(CORE_DIR, 'ovldrop.txt'), '超限锻造石掉落（rr/overlimit.py）：tier 当前档标记 Boss颗数 小怪几率%；goods；range；boss 击败标记 各档掉过标记；bossent；nodrop；ash 各档骨灰个数；ashes 随机挑的原版骨灰',
              overlimit.drop_lines(ctx))
    # 融锻之心的 Boss 掉落（dark_core 的 fusion 模块：平时追忆 Boss 按难度递增、死亡 / 不可能里所有 Boss 照龙心脏的数量，各只给一次）
    write_def(os.path.join(CORE_DIR, 'fusion.txt'), '融锻之心、许愿石的 Boss 掉落（rr/bosses.py fusion_lines）：goods 融锻之心（0 = 不给）；wish 许愿石；lock 死亡以上的当前档标记；'
              'runefloor 死亡以上 Boss 卢恩保底；rem 击败标记 数量 平时给过的标记（追忆 Boss）；boss 击败标记 融锻之心数量 死亡以上给过的标记 许愿石数量 原版击败奖励', bosses.fusion_lines(ctx))
    # 许愿石升级（dark_core 的 wishstone 模块：赐福菜单点亮请求，扣 1 个许愿石、能力值 +1、等级 +1）
    write_def(os.path.join(CORE_DIR, 'wishstone.txt'), '许愿石（rr/wishstone.py）：goods；req 请求标记；attr 能力值序号起点 位数；none 没有许愿石；max 已经 99',
              wishstone.def_lines(ctx))
    # 魇化精英与魇印（dark_core 的 nightmare 模块）
    write_def(os.path.join(CORE_DIR, 'arena.txt'), '斗技场试炼（rr/arena.py）：req / shard / goods0 / local 标记；per / items / cat；diff / dmult；run 场次；'
              'colo / statue / rar / ashrar / part 各竞技场；note 竞技场里的提示；wfx 按波次挂的效果包', arena.def_lines(ctx))
    write_def(os.path.join(CORE_DIR, 'foefx.txt'), '敌人效果包（rr/foefx.py）：pack 包名 动作速度 效果,效果…（- = 没有）', foefx.def_lines(ctx))
    write_def(os.path.join(CORE_DIR, 'nightmare.txt'), '魇化精英与魇印（rr/nightmare.py）：tier 当前档标记 魇化几率% 两条几率% 魇火稀有度 精英掉石几率%；hp / color / fam；'
              'ember_max；range；levels；slots；count0 / slot0 标记起点 位数；goods；mobe 实体… / mobn 敌人参数…', nightmare.def_lines(ctx))
    # 原版闪烁提示（dark_core 的 notes 模块）
    write_def(os.path.join(CORE_DIR, 'notes.txt'), '原版闪烁提示（rr/notes.py）：bits 编号按位写进的标记；note 键 编号', notes.def_lines(ctx))
    # 暗黑商店分阶段解锁（dark_core 的 shopstage 模块：打败有新东西上架的区域的追忆 Boss 时弹提示）
    write_def(os.path.join(CORE_DIR, 'shopstage.txt'), '暗黑商店分阶段解锁（rr/darkshop.py stage_lines）：note 提示的键；gate 追忆 Boss 击败标记 区域',
              darkshop.stage_lines(ctx))
    # 咒痕（dark_core 的 curses 模块）
    write_def(os.path.join(CORE_DIR, 'curses.txt'), '咒痕（rr/curses.py）：tier；listen；shards；region 序号 诅咒 碎片标记 名字；text；burn / gaze / erode / brand / vary / gloom / echo；boss 击败标记 区域；unlock 存档解锁的标记',
              curses.def_lines(ctx))
    # 词条疫病（dark_core 的 plague 模块：戴着时给在场的敌人摘免疫效果、挂抗性倍率效果）
    write_def(os.path.join(CORE_DIR, 'plague.txt'), '词条疫病（rr/plague.py）：mark / raise 各稀有度的标记效果、敌人抗性加多少 %；status 序号 免疫效果 抗性倍率效果起点 行数；n 敌人参数 中毒第几行 腐败第几行 位（参数里挂着免疫效果）',
              plague.def_lines(ctx))
    # 无形（dark_core 的 unseen 模块：戴着时让敌人不对箭、法术、道具的子弹作出反应）
    write_def(os.path.join(CORE_DIR, 'unseen.txt'), '无形（rr/unseen.py）：mark 词条标记；talisman 护符；b 箭、法术、道具的子弹', unseen.def_lines(ctx))
    # 一键出售（dark_sell）能卖的：新装备系统的全部容器（类别 编号）
    write_def(os.path.join(SELL_DIR, 'sell_items.txt'), '一键出售能卖的装备：类别（0 武器 / 1 防具 / 2 护符）、编号（武器 +0）', containers.sell_list(ctx))
    # 复制战灰（dark_sell）：清单每格按背包里实际有的战灰开关
    write_def(os.path.join(SELL_DIR, 'ash_dup.txt'), '复制战灰：战灰编号、这一格的解锁标记（dark_sell 按背包和仓库里实际有的战灰开关）', darkshop.ash_dup_lines(ctx))
    # 以前的：容器验证
    write_def(os.path.join(CORE_DIR, 'container.txt'), '', [])


def write_and_check(ctx, t0):
    a, bnd = ctx.a, ctx.bnd
    log('  [3/4] 写回 regulation...')
    bnd.put('ItemLotParam_enemy.param', ctx.P_en.serialize())
    bnd.put('ItemLotParam_map.param', ctx.P_mp.serialize())
    out_reg = os.path.join(a.out, 'regulation.bin')
    bnd.compact()
    # 文字包（各步骤改来改去，只留在内存里）这时才一起并行压缩写出；主进程同时写 regulation
    erlib.flush_writes(while_waiting=lambda: erlib.write_regulation(out_reg, bytes(bnd.data)))
    skillgems.place_anim(ctx)                 # 改了判定号的动作包：regulation（带复制的判定行）写好了才放

    log('  [4/4] 校验...')
    chk = erlib.Bnd4(erlib.read_regulation(out_reg, strict=True))  # 按游戏的固定偏移方式读回
    if len(erlib.Param(chk.get('ItemLotParam_enemy.param')).rows) != len(ctx.P_en.rows) or \
            len(erlib.Param(chk.get('ItemLotParam_map.param')).rows) != len(ctx.P_mp.rows):
        raise RuntimeError('校验失败：掉落表行数和写出去的不一致')
    if not a.noop:
        affixes.verify(chk, getattr(ctx, 'patched_rows', None))
    json.dump(ctx.features, open(os.path.join(a.out, 'features.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
    if not a.noop:
        write_dll_defs(ctx)
    info = ['暗黑重铸 重随报告', '时间：%s' % time.strftime('%Y-%m-%d %H:%M:%S'), '种子：%d' % ctx.seed,
            '基础：%s' % a.base_reg, '文字：%s' % ' / '.join(a.msg_src), '标签：%s' % a.label]
    info += ctx.report + ['用时 %.1f 秒' % (time.time() - t0)]
    rep_path = os.path.join(a.out, 'reroll_report.txt')
    open(rep_path, 'w', encoding='utf-8').write('\n'.join(info) + '\n')
    # 启动日志里只留有问题的那几行（没加上 / 跳过 / 失效…），完整报告在 reroll_report.txt（诊断包会带上）
    for line in ctx.report:
        if any(k in line for k in PROBLEM_WORDS) and not re.search(r'(?<![0-9.])0 (个|件|格|处|把)[^，；]*(' + '|'.join(PROBLEM_WORDS) + ')', line):
            log('    ' + line.strip())
    # 种子不打在启动日志里（只在报告里）：玩家会当成「又换了个种子重新随机化敌人」（2026-11-04 用户反馈），其实只是重随包自己的随机数
    log('  完成（用时 %.1f 秒；完整报告 %s）' % (time.time() - t0, rep_path))


def main():
    t0 = time.time()
    a = parse_args()
    erlib.DLL_DIRS = [d for d in a.dll_dir if d]
    # 输入都没变：沿用上次的重随包（DLL 的定义文件对不上就写回去），不重做（rr/reuse.py）
    dirs = {'core': CORE_DIR, 'sell': SELL_DIR}
    pre = None if a.noop else reuse.inputs(a, HERE, LANGS)
    rec = reuse.check(a, pre, dirs) if pre else None
    if rec:
        n = reuse.restore_defs(rec, dirs)
        reuse.touch_report(a, rec)
        log('  输入没变，沿用上次的重随结果（%s 做的%s；用时 %.1f 秒）' % (
            rec.get('time', '?'), '；DLL 定义文件写回 %d 个' % n if n else '', time.time() - t0))
        return
    reuse.clear(a)
    bar = progress.Bar(HERE)          # 给启动器画进度条（启动器设 DR_PROGRESS=1 时才打印，见 progress.py）
    rg = bar.ranges(['load', 'run_steps', 'write_and_check'])
    erlib.defer_writes(True)          # 文字包先放内存，write_and_check 里一起写出
    with bar.track(load, *rg['load']):
        ctx = load(a)
    if not a.noop:
        with bar.track(run_steps, *rg['run_steps']):
            run_steps(ctx)
    with bar.track(write_and_check, *rg['write_and_check']):
        write_and_check(ctx, t0)
    bar.save()
    if pre:
        reuse.save(a, pre, dirs, DEF_PATHS, ctx.seed, time.time() - t0)


if __name__ == '__main__':
    try:
        main()
    except Exception as e:
        print('  [错误] 重随失败：%s: %s' % (type(e).__name__, e), flush=True)
        sys.exit(1)
