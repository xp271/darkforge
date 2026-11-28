# -*- coding: utf-8 -*-
"""从 RNG Lands 原包（packs\\Ruby\\recommended increased difficulty）提取词条库，存成我们自己的数据（以后 packs\\ 删掉也能用）。
开发工具，只在要重新提取时跑：
  launcher\\python\\python.exe -X utf8 dev\\item_system\\extract_affixes.py [--pack <包目录>]

按 词条一览.md 的决定合并（数值不改，合并的取件数加权最常见的那档）：
  护符 / 防具：魔法流派 11 条 → 「魔法伤害」；祷告流派 7 条 → 「祷告伤害」（咆哮与吐息单独留）；
              魔力 / 火 / 雷 / 圣单属性减伤 → 并进「魔力、火、雷、圣减伤」；其余照旧。
  武器：命中异常按 7 种（出血、毒、腐败、冻伤、睡眠 + 新加咒死、发狂）；法杖 / 圣印记流派加成保留；
        盾牌的异常抗性合并成一条；弓、击杀回复、专属被动、没有效果的删；传说光效按属性配；补正按 第四节 的规则。

输出到 launcher\\reroll\\data\\affixes\\：
  affixes.json  词条定义和生成用的分布（权重、词条数、补正、异常、战技……）
  rows.json     原版里没有、要插进 regulation 的整行参数（十六进制）：RNG Lands 加的效果 / 行为 / 子弹 / 攻击，
                合并出来的新效果，护符模板
  提取报告.txt   给人看的摘要"""
import argparse
import json
import math
import os
import re
import struct
import sys
import time
from collections import Counter, defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
PALY = os.path.dirname(os.path.dirname(HERE))
RR = os.path.join(PALY, 'launcher', 'reroll')
sys.path.insert(0, RR)
import erlib  # noqa: E402
import paramdef_min as pdm  # noqa: E402

OUT = os.path.join(RR, 'data', 'affixes')
LOCK = os.path.join(HERE, 'id_lock.json')  # 编号锁定（Extract.pin）：装备记录 / 存档里存的编号按名字固定
VANILLA = os.path.join(PALY, 'randomizer', 'diste', 'Vanilla')
RAR = ('Common', 'Rare', 'Epic', 'Legendary')
SEP = '\n-----------------------------\n'
CAT = {'WeaponName': 'w', 'ProtectorName': 'p', 'AccessoryName': 'a'}
NUM = re.compile(r'[-+]?\d+(?:\.\d+)?')

# 新建的行放这些段（原版和原包都没用）
NEW_SP = 7200000           # 合并出来的效果：魔法 / 祷告伤害、盾牌抗性、咒死 / 发狂的新强度
NEW_ACC = 9000             # 护符模板：9000 起按词条 × 稀有度重新编号（原包的 RNG 护符 9000~12999 不再存在）
# 触发类词条补齐稀有度（tier_triggers）新建的行
TIER_SP, TIER_BEH, TIER_BULLET, TIER_ATK, TIER_GROUP = 7210000, 600004000, 93000000, 307000000, 11200
TIER_DMG = {'Common': 0.45, 'Rare': 0.6, 'Epic': 0.8, 'Legendary': 1.0}   # 伤害 / 治疗 / 增伤按稀有度的比例（相对传说）
ONHIT_STATE = 303          # 命中触发：stateInfo 303~306 = 普通~传说（同稀有度的命中效果不叠加）
# 词条数值随机（用户 2026-11-15「数值类型词条的数值变为一个随机范围 截断正态 每个稀有度之间重叠20%」，「触发链类全部不随机」「轻链一起做」；
# 同日「不能是占位效果行然后改写内存吗」：不再每个取值建一行，dark_core 按身上装备把精确数值写进占位行再挂上）：
# 数值写在效果行本身（或只连着几行效果、没有子弹 / 攻击）的词条，每个稀有度的范围 = 相邻两档的中点再往外各扩到相邻重叠 VAL_OVERLAP，
# 按截断正态连续抽（σ = 中心到较远一端的一半，均值调到正好是现在的数），按显示精度取整（字段是浮点的 0.1，整数字段 1）。
# 装备记录存「在这档范围里的位置」（-1 = 下端，0 = 现在的数，1 = 上端），调了数值装备跟着变；老装备没有就是 0。
# 词条库 gear[键]['vrand']：主数 / 跟着插值的数、各档范围和抽样参数、说明模板（{序号} 是变化的数）、范围说明（词条百科），
# 和 dark_core 写占位行要的：这一档效果链的行（四档共用的行照旧引用）、链内引用的字段偏移、和数成干净线性关系（1 ± 0.01N、±N、±10N、±0.1N）的字段偏移。
# 其他按稀有度不同的字段（命中触发的 stateInfo 等）照这一档的
VAL_OVERLAP = 0.2
# 数值随机的范围上限（主数不超过这个）：咏唱加速的虚拟灵巧（用户 2026-11-18「虚拟灵巧的词条上限改到99」；传说原来 84~114，超过 99 的白给）。
# 现在的数正好在上限上的那一档（传说 99）：μ 就是它、σ 照下端算（均值调不到它了），抽出来多数靠近上限
VAL_CAP = {'adds_virtual_dex_toward_speed_casting': 99}
# 二期（value_ranges_rules）：积木规则里能按位置取精确数的字段（每档一个数；dark_core rules.rs 的 set_num 要认得）、
# 规则里按档 / 按层挂的效果编号的字段（rules.rs 的 set_id；行里的数由积木引擎写进占位行）、专用模块（runtime.rs）定义里数值的位置
VAL_RULE_NUMS = ('stamina', 'fp', 'hp_pct', 'fp_pct', 'burst', 'burst_flat', 'chance', 'haste', 'take_pct', 'cooldown', 'hp_flat', 'foe_secs',
                 'act_speed', 'reload_speed')
VAL_RULE_SLOTS = ('effect', 'fx', 'mfx', 'target_fx')
VAL_RUNS = {'deft': (0,), 'quickcharge': (0,), 'timeslip': (0,), 'loadspeed': (0,), 'crush': (0,), 'magnet': (0,)}
STEP_SKIP = ()               # 不随机的词条键（调平衡用）
STEP_RUNTIME_OK = ('dr_blood_drink',)   # 新词条里效果是游戏原生的（STACK_NATIVE_NEW）；别的新词条 dark_core / 重随的模块按稀有度认标记，不随机
STEP_FORMS = ((0.0, 1.0), (0.0, -1.0), (1.0, 0.01), (1.0, -0.01), (0.0, 0.01), (0.0, -0.01), (0.0, 10.0), (0.0, -10.0), (0.0, 0.1), (0.0, -0.1))
STEP_SPR = ('replaceSpEffectId', 'cycleOccurrenceSpEffectId', 'atkOccurrenceSpEffectId', 'accumuOverFireId', 'accumuUnderFireId',
            'spiritDeathSpEffectId', 'applyIdOnGetSoul')
TAL_SORT0 = 410000         # 护符模板的 sortId：TAL_SORT0 + 词条序号 × 10 + 稀有度（sort_templates；原版护符 400000~407070，排在它们后面）
ATK_ELEM = (('atkPhys', '物理', 'Physical'), ('atkMag', '魔力', 'Magic'), ('atkFire', '火', 'Fire'),
            ('atkThun', '雷', 'Lightning'), ('atkDark', '圣', 'Holy'))
CD_LINE = (('效果每 # 秒最多触发一次', '效果结束前不会再次触发'),
           ('Effect can trigger once every # seconds', 'Cannot trigger again until the effect ends'))
# 原包英文说明数字和实际不符的（中文已按参数改过）：第一行按中文的数字重写
EN_FIX = {
    'repeatedly_hitting_enemies_with_melee_attacks_ch': 'Repeatedly hitting enemies with melee attacks charges your weapon with medium glintstone power. '
                                                         'Once fully charged, for # seconds spawns small glintstone shards every # seconds that assault enemies.',
    'repeatedly_hitting_enemies_with_melee_attacks_pl': 'Repeatedly hitting enemies with melee attacks places a Terra Magica area on the ground '
                                                         'that raises magic damage by #% (#% against players)',
}
DROP_GEAR = ('harness_power_of_crucible_knights_increase_i_fra',)   # 熔炉骑士：原版熔炉全护符的特效，数值在引擎里分不了稀有度（2026-10-27 删）
# 数值调整：词条 →（效果里的倍率字段, 加成部分乘多少）。削韧词条减半是 v0.4 定的，迁移时补回（2026-10-27）
RESCALE = {'increase_posture_damage_by_pct': ('saAttackPowerRate', 0.5)}
# 数值直接定：词条 →（效果里的倍率字段, {稀有度: 百分比}），字段都改成 1 + 百分比 / 100，说明里的数字跟着换。
# 强韧力气（血量全满时伤害）4 / 6 / 9 / 12 → 7 / 10 / 15 / 20（用户 2026-11-02「强韧力气词条伤害提升至20%」，其他档按同比例 ×5/3 取整）
ATK_RATES = ('physicsAttackRate', 'magicAttackRate', 'fireAttackRate', 'thunderAttackRate', 'darkAttackRate')
# 满蓄力（蓄力魔法 / 祷告 / 战技）4 / 6 / 9 / 13 → 6 / 9 / 14 / 20（用户 2026-11-13「满蓄力数值提升到20%」，其他档同比例取整）
# 延迟魔法（魔法、祷告的有效时间）5 / 8 / 11 / 15 → 33 / 53 / 73 / 100（用户 2026-11-20「魔法持续时间延长那个词条传说改为+100% 其他缩放」，都 ×100/15 取整）
RESET = {'increase_dealt_damage_when_at_full_hp_by_pct': (ATK_RATES, {'Common': 7, 'Rare': 10, 'Epic': 15, 'Legendary': 20}),
         'increase_damage_of_fully_charged_skills_or_spell': (ATK_RATES, {'Common': 6, 'Rare': 9, 'Epic': 14, 'Legendary': 20}),
         'increase_spells_duration_by_pct': (('extendLifeRate',), {'Common': 33, 'Rare': 53, 'Epic': 73, 'Legendary': 100})}
# 直接定数值的（字段本身就是这个数，不是倍率）：词条 → (字段, {稀有度: 数值})。咏唱加速：用户 2026-11-06「虚拟灵巧那个词条传说改为+99」「其他稀有度缩放一下」（字段 s8，以前 20 / 28 / 38 / 50，都 ×99/50）
RESET_VAL = {'adds_virtual_dex_toward_speed_casting': ('dexterityCancelSystemOnlyAddDexterity', {'Common': 40, 'Rare': 55, 'Epic': 75, 'Legendary': 99})}
# 满蓄力只管蓄力魔法、祷告（用户 2026-11-27「满蓄力可以把战技伤害单独去掉吗」→「改」）：效果行认的攻击子类别 110（蓄力魔法 / 祷告，
# 原版攻击参数里 155 行全是法术）留着，111（蓄力战技，68 行全是战技）清掉；说明跟着改，流派按新说明归「法术」（ARCHETYPES）
CHARGED_SPELL = dict(key='increase_damage_of_fully_charged_skills_or_spell', keep=110, drop=111,
                     zh='蓄力使用魔法、祷告时的伤害＋#％', en='Increase damage of fully charged sorceries and incantations by #%')
# 词条改造：词条 →（后缀（英, 中）、改回 1 的倍率字段（去掉这部分效果）、新说明（英, 中；# 按原说明的数值顺序填））
# 斩击优化 → 战技优化，去掉「精力上限降低」（用户 2026-10-31）；咏唱优化去掉「血量上限降低」（用户 2026-11-01）
REWORK = {'decrease_cost_of_ashes_of_war_and_decrease_max_s': (('optimized skills', '战技优化'), ('maxStaminaRate',),
                                                               ('Decrease FP cost of weapon skills by #%', '战技消耗的专注值减少#％')),
          'decrease_cost_of_spells_and_incantations_and_dec': (('optimized casting', '咏唱优化'), ('maxHpRate',),
                                                               ('Decrease FP cost of spells and incantations by #%', '魔法、祷告消耗的专注值减少#％'))}
# 狂怒突刺（用户 2026-11-06「修一下」）：原包的效果行只有 stateInfo 483，真正生效的是原版致命一击攻击（AtkParam_Pc）挂的 20382301 → 20382302，
# 各稀有度都是对敌伤害 +20%、20 秒，和说明的 6 / 9 / 12 / 16%、40 秒不符；原包那几行数值没有人引用（词条库也没带）。
# 现在标记换成空行（去掉 483，原版那条链就不触发了），积木引擎在致命一击结束时挂这里的加成
CRIT_RAGE = dict(key='increase_attack_by_pct_for_seconds_after_critica', secs=40, vals=(6, 9, 12, 16),
                 zh='施展致命一击后的#秒内，能提升#％攻击力\n同名词条只有稀有度最高的生效',
                 en='For # seconds after a critical hit, attack power is raised by #%\nOnly the highest rarity of this effect applies')
# 强韧力气（用户 2026-11-06「改为80%血以上触发」，原来要血量全满，「死亡」里焚魂一直掉血就触发不了）：conditionHpRate = 血量不低于这个百分比才生效
HIGH_HP = dict(key='increase_dealt_damage_when_at_full_hp_by_pct', rate=80.0, zh='血量在80％以上时，造成的伤害＋#％',
               en='While HP is at 80% or above, damage dealt is raised by #%')
# 致命舞（用户 2026-11-06）：装备重量越轻，所有动作越快；空载时是这里的百分比，满载时 0（Extract.load_speed；同名取最高稀有度，不做同稀有度叠加的副本）
LOAD_SPEED = dict(key='increase_damage_with_lower_equip_load_damage_inc', vals=(15, 20, 25, 30),
                  zh='装备重量越轻，所有动作越快（最高＋#％）\n同名词条只有稀有度最高的生效',
                  en='The lighter your equip load, the faster all your actions (up to +#%)\nOnly the highest rarity of this effect applies')
# 道具伤害统一（用户 2026-11-01）：reroll 给投掷壶 108、调香 109、飞刀 / 流纹武器投掷 119、DLC 大壶 131 和石头等道具的攻击行
# 都加上子类别 108（launcher\reroll\rr\tweaks.py item_damage），词条只认 ITEM_CAT。「坚硬壶」那条改成「道具伤害」留着抽；
# 「调香」「投掷」两条停用（不再抽到），效果行照留下那条同稀有度的（数值、图标一样），已有装备跟着变
ITEM_CAT = 108
ITEM_RATES = ('physicsAttackRate', 'magicAttackRate', 'fireAttackRate', 'thunderAttackRate', 'darkAttackRate')   # 原版道具 / 箭矢伤害护符改的攻击倍率
ITEM_DMG = dict(key='increase_raw_damage_of_thrown_pots_by_pct', cats=(108, 109, 119),
                retire=('increase_damage_dealt_by_perfumes_by_pct', 'increase_damage_of_throwing_attacks_by_pct'),
                suffix=('Items', '道具'), zh='道具伤害＋#％', en='Increase item damage by #%')
# 武器类型（wepType）→ 原版 EquipParamGem 里「能装这类武器」是 canMountWep_Dagger 那一字节起的第几位（参数定义按顺序排，
# 大枪 28 是 SpearHeavy、DLC 类型 88~95 接在 Torch 后面的 36~43 位；2026-10-31 按原版能换战灰的武器和它们默认的战灰核对过）
MOUNT_BIT = {1: 0, 3: 1, 5: 2, 7: 3, 9: 4, 11: 5, 13: 6, 14: 7, 15: 8, 16: 9, 17: 10, 19: 11, 21: 12, 23: 13, 24: 14, 25: 15, 28: 17,
             29: 18, 31: 19, 35: 20, 37: 21, 39: 22, 41: 23, 50: 24, 51: 25, 53: 26, 55: 27, 56: 28, 57: 29, 61: 31, 65: 32, 67: 33,
             69: 34, 87: 35, 88: 36, 89: 37, 90: 38, 91: 39, 92: 40, 93: 41, 94: 42, 95: 43}
# 持续掉血改成按最大血量的百分比：词条 → 每秒 %（扣血的是各稀有度效果 cycle 出来的那行）。剧毒注射原来每秒扣 12 点，统一成每秒 1％（用户 2026-10-31）
HP_DRAIN_PCT = {'when_poisoning_or_rot_occurs_in_vicinity_increas': 1.0}
# 霜铠（RNG 包里没有的附近触发词条，v0.4 加的霜铠护符迁移过来；机制参照 ELDEN VINS 的 Caligo's Dream）：
# 冻伤发作效果（stateInfo 260、spCategory 10007）接 cycle → 放子弹 → 范围内挂标记（stateInfo 578，原版没人用）→ 护符效果听 578 挂增益。
# 判定链全用原版行复制（腐败眷属的欢愉那条：505 / 506 / 行为 2105 / 子弹 1005 / 攻击 2 / 321700 / 321701 / 特效 321700）；
# 冻伤发作行是原版行，rows.json 不覆盖原版，改成 affixes.json 的 hooks 由重随时接上
RIME_KEY = 'when_frostbite_occurs_in_vicinity_reduce_damage'
RIME_TIERS = ((7, 5), (10, 8), (14, 11), (20, 16))      # 普通~传说：(受到伤害降低 %, 精力恢复速度 +)
RIME_SEC, RIME_STATE, RIME_WEIGHT = 40.0, 578, 40      # 持续秒数、标记的 stateInfo、护符权重（原包词条的中位数）
RIME_ICON = 18170                                      # 护符图标（原版健壮角饰品的）
# 增益拆成两行、各带一个图标，冻伤时一起挂（用户 2026-11-19「可以同时触发两个图标吗」）：减伤用原版防御提升（20408，溢出的露滴、水煮螃蟹），
# 精力恢复用原版精力恢复（20410，腌乌龟脖子、迸发结晶露滴）。两行各由一个监听挂（护符 refId + residentSpEffectId1），同一个标记触发
RIME_CUT_ICON, RIME_STAM_ICON = 20408, 20410
RIME_MORE_SP = 7218000                                 # 第二个监听 / 精力恢复那行的号段（每档 2 个，锁定；TIER_SP 的游标到不了这么远）
RIME_LIKE = 'when_blood_loss_occur_nearby_increase_damage_by'   # 护符模板照这个词条的（同为附近触发）
RIME_TEXT = {'zh': '周围发生冻伤时，%d秒内受到的伤害减少%d％，精力恢复速度＋%d\n同稀有度仅生效一次',
             'en': 'When FROSTBITE occurs in vicinity, reduce damage taken by %d%% and increase stamina recovery by %d for %d seconds\n'
                   'Stacks only once per rarity'}
CUT_FIELDS = ('slashDamageCutRate', 'blowDamageCutRate', 'thrustDamageCutRate', 'neutralDamageCutRate',
              'magicDamageCutRate', 'fireDamageCutRate', 'thunderDamageCutRate', 'darkDamageCutRate')
ATK_RATES = tuple('atk%sDmgCorrectRate_%s' % (w, e) for w in ('Player', 'Enemy') for e in ('Physics', 'Magic', 'Fire', 'Thunder', 'Dark'))
STATUS_DEF = ('poisonDefDamageRate', 'diseaseDefDamageRate', 'bloodDefDamageRate', 'curseDefDamageRate', 'freezeDefDamageRate',
              'sleepDefDamageRate', 'madnessDefDamageRate')     # 防御方：受到的异常累积倍率
DEF_RATES = tuple('def%sDmgCorrectRate_%s' % (w, e) for w in ('Player', 'Enemy') for e in ('Physics', 'Magic', 'Fire', 'Thunder', 'Dark'))
# 受击等级替换（值 = 等级 + 1，1 = 无硬直，0 = 不替换）
DMG_LV = tuple('dmgLv_' + n for n in ('None', 'S', 'M', 'L', 'BlowM', 'Push', 'Strike', 'BlowS', 'Min', 'Uppercut', 'BlowLL', 'Breath'))
# 新词条（2026-10-27 用户给的表，原包没有）。装备 / 护符上只挂一个「标记」效果（照长尾猫护符 360400 复制、去掉效果），
# 真正的效果按 run 由玩家脚本（launcher\reroll\sc\hks.py，按标记判断稀有度）或 dll\dark_core（runtime.rs）做：
#   gale 疾风：移动速度（玩家脚本 Move_onUpdate 的 SetMovementScaleMult）；wolf 狼魂：攻击中随时防御取消（ExecGuardOnCancelTiming），消耗精力；
#   leap 飞鸿：起跳时挂一段计时标记，dll\dark_core 在标记期间设翻滚同款的无敌位；overlord 霸王：一次性霸体，战技状态里每帧挂照「忍耐」1650 复制的短效果（光效、图标、减伤），挡硬直由玩家脚本做；
#   elegance 优雅：做同一个姿势满 1 秒记一层（每种姿势一层），20 秒；DLL 按层数换效果（每层一行）；
#   cocktail 鸡尾酒：每 10 秒随机挂一种正面效果，持续 10 秒（DLL）；
#   timeslip 时隙：周围 8 米（照授血仪式的范围）内所有角色不分敌我，动画速度 ×(1 + 数值%)（DLL 每帧改角色行为模块的 animation_speed）。
# slots：能出在哪（talisman / head / body / arm / leg）；vals：普通 ~ 传说的数值
SLOT_BIT = {'head': 1, 'body': 2, 'arm': 4, 'leg': 8}
# 2026-11-04 用户「平均一下 然后让各个流派错开」：只出一个部位的前缀词条每部位 7~8 条、同流派分到不同部位（泡盾 / 浮空 → 头，壁垒 / 坠星 / 碾压 / 转念 → 身，
# 凌空 → 手，沉稳 / 反震 / 引爆 → 腿）；只影响以后生成的防具

# 积木词条（bd设计.md 6.5）：效果行和积木规则都写在定义里（rule_affixes），不再按词条写专用代码；dll\dark_core 的 rules.rs 照规则做。
#   rows：{行名: 函数(这个稀有度的数值) → 字段（照 360400 空效果复制后改；src= 换底行）或字段的列表（叠层：每层一行）}
#   logic：[{'type': …}]，参数值是函数的按稀有度算，是行名的换成这行的编号（各稀有度）；事件名见 sc/hks.py 的 DR_Event
#     while：条件（持续状态事件）成立时一直挂 effect；on：事件发生时执行（fp / stamina / hp 加减、挂 effect），cooldown 秒；
#     stack：add 事件加一层、clear 清零、consume 等当前动作做完再清零，max 上限、timeout 秒没加层清零，fx 每层的效果，
#            stamina / hp：每次加层的加减；log：写 dark_core.log 的名字
#   rarities：只在这些稀有度出；keystone：规则改变器（护符放进 KEYSTONE_GROUP，游戏本身不让同组护符一起装）
#   own_group：护符自己一个组（OWN_GROUP 起，比 KEYSTONE_GROUP 优先），和其他护符都不冲突（无形，用户 2026-11-19）
#   while / stack 挂的效果写 effectEndurance=-1（不限时，引擎负责摘）：限时的效果快到期时状态图标会一直闪（用户 2026-10-28 报的）
#   老的 CUSTOM 条目也能写 rows / logic（比如霸王的图标、光效）
KEYSTONE_GROUP = 11190
OWN_GROUP = 11191       # own_group 的护符从这里起各占一个组，要小于 TIER_GROUP
ICON_ATK, ICON_STAM = 20406, 20410
# 人物身上的光效（SpEffectVfxParam，原版的；用户 2026-10-28 要加）：引擎每 0.5 秒续一次效果，持续挂的选没有起手特效的，免得一直闪
VFX_CALM = 5352        # 腌制龟颈肉（精力恢复，绿）
VFX_RAGE = 5350        # 勇者肉块（攻击力提升，红）
VFX_FP = 104901        # 蓝露滴圣杯瓶喝下时的特效（道具的 effectSfxId 301052，原版光效行 104901 的持续特效就是它）：转念回专注时挂 1.5 秒（用户 2026-10-28）
VFX_BREAK = 8725       # 原版加攻击的效果 1586 的光环
VFX_FLOW = 8620        # 黄金誓约（金）
VFX_BULWARK = 5370     # 煮熟螃蟹（减伤，带开头特效）：壁垒每加一层换一行，开头特效播一次
# 物理冲击波（碾压 / 反震 / 坠星共用的 24 档子弹）的特效：死之鸟（c3250）拐杖下砸的冲击（子弹 3250141 等，范围 0.1 → 3.5 米、
# 存活 0.2 秒、朝向 1）。用户 2026-10-28 要的；它在常驻的公共特效包 sfxbnd_commoneffects 里（632515 → 子特效 690004，
# 贴图都在同一个包），死之鸟不在场也能放。跳起下砸是 632516（→ 690008，6.5 米，比我们 2.5 米的判定大太多）。
# 存活 0.5 秒、命中记录 99 秒（每个敌人只挨一下）。以前用过：特大剑「遗迹大剑」战技砸地 450116、战灰「暴风践踏」449990；
# 熔岩土龙 boss 的砸地（6491xx）是 boss 专属特效包，不在场不加载。
# 用户 2026-10-28：特效比实际范围大，要缩小到一致。这套砸地冲击是按大小分档的同一种特效（常驻包里，按原版用它们的子弹判定半径推的）：
# 690000 / 690001 约 2~2.5 米、690002 3 米、690003 / 690004 3.5 米（死之鸟拐杖下砸）、690005 4~4.5 米、690008 6.5 米；
# 我们的判定半径 2.5 米，换成调用 690001 的壳 645610（和死之鸟的 632515 同样大小、同样写法，只是调用的档不同）
SHOCK_SFX = 645610
SHOCK_BULLET_FIELDS = dict(sfxId_Bullet=SHOCK_SFX, sfxPostureType=1, life=0.5, dmgHitRecordLifeTime=99.0)
VFX_SURGE = 320400     # 红羽七刃剑（血量低时攻击力提升）：血涌
SPELL_RATES = ('physicsAttackRate', 'magicAttackRate', 'fireAttackRate', 'thunderAttackRate', 'darkAttackRate')
ICON_POISE = 20465
VFX_MIKIRI = 8740      # 黄金的光（效果 1835 / 1836，带开头特效）：见切时挂 1 秒
ICON_ORB = 20207       # 魔法师球护符
ENEMY_LIKE = 880       # 原版冻伤发作：命中时挂给对方的效果只借它的目标位（enemy_sp）
DIRECT_TARGET_BITS = ('effectTargetSelf', 'effectTargetSelfTarget', 'effectTargetAI', 'effectTargetLive')   # dark_core 直接挂的要有（allow_direct）
FOE_HIT_MARKS = ('shot_hit', 'item_hit', 'cast_hit')   # 挂在被打的敌人身上的命中标记（子弹命中时挂；dark_core rules.rs 的 FOE_HIT_KINDS）
FREE_EVENTS = ('shot', 'shot_new', 'throw', 'throw_new')   # 无尽箭袋（free 规则）听的：射击 / 投掷的状态和新的一次
VFX_HEAL = 5071101     # 蛇神弯刀打倒敌人回血的一闪（只有开头特效 320160）：渴血回血时挂
VFX_WARD = 5866        # 珍珠泡状露滴（泡泡护盾）：无敌期间
ICON_BLEED = 20285     # 鲜血君王的欢愉（周围出血）
VFX_WEAKEN = 1709000   # 桂奥尔的咆哮挂给敌人的降攻 / 降防（效果 1709000）：天堑在地上降攻时挂
# 自制特效（make_sfx.py 打进我们自己的 sfxbnd_c0000，原版没有这个文件）：光效行编号 → (特效编号, 挂点)，照 1709000 复制、只换特效和挂点
# 猎印的准星 1~3 层（浅绿 → 深绿，用户 2026-10-28），挂在锁定点 220（原版光效行最常用的身体中间挂点）
VFX_HUNT = (990001, 990002, 990003)
# 环绕法球（凝聚）1~4 颗：辉石魔砾绕着人物转（用户 2026-10-29：参考原版辉石魔砾，有几个法球就有几颗），挂在身体中间 220
# 5~7 颗（2026-11-04 盈满按稀有度多 1~4 颗，凝聚最多 3 颗 → 最多 7 颗）
VFX_ORBIT = (990011, 990012, 990013, 990014, 990015, 990016, 990017)
ORB_BURST = (90, 135, 180, 240)        # 迸发每颗法球的伤害；星群用最高档（用户 2026-11-06）
DETONATE_GROW = 2.5                    # 引爆：每多一层猎印伤害乘多少（用户 2026-11-13，以前 1.5）
ORB_GROW = 10                          # 迸发、星群每颗法球再加多少 %（用户 2026-11-13「法球每颗增加伤害改为10%吧」，以前 15）
ORB_CAST_SPEED = 5                     # 星群每颗法球加快多少 % 施法（用户 2026-11-06）
HAWK_ORBS = (1, 2, 3, 4)               # 盈满（键 dr_hawk，原名鹰眼视觉）各稀有度多几颗法球
ORB_MAX = 3 + max(HAWK_ORBS)           # 法球最多几颗（凝聚传说 3 颗 + 盈满传说 4 颗）
CUSTOM_VFX = {v: (v, 220) for v in VFX_HUNT + VFX_ORBIT}
# 相杀受击时的火花（用户 2026-11-04「特效直接用原版防御火花」）：金属武器防御（受击材质 106）被金属武器打中时的火花 201001
# （HitEffectSfxConceptParam 106 → HitEffectSfxParam 10 的 Normal），常驻特效。挂在武器上一个点（用户 2026-11-04「改挂到武器上」「挂一个点就行」）：
# 武器模型的挂点 130 = 打击部位的中间（长剑 0.53 / 刀身 0.09~0.90，大剑 0.77 / 0.13~1.20，匕首、刀、矛头都在中间；看过原版模型的 FLVER 挂点）。
# 300 → 301（→ 302 / 303）是刀根到刀尖的一条线，310 / 320 / 330 只有多刃的特殊武器才有，所以不用它们。
# 照原版武器光效 8575 / 8576：右手武器 10130、左手 20130（10000 / 20000 段 = 右 / 左手武器模型上的挂点），effectType 1 / 2
# 2026-11-04 用户「改一下火花特效 现在太不明显了」：换成放大加亮的副本 990040（dev\item_system\sfx\clash_spark.js，在我们的 sfxbnd_c0000）
VFX_CLASH_R, VFX_CLASH_L, CLASH_SPARK_SFX = 990031, 990032, 990040
# 2026-11-04 用户「或者换到护手的位置吧」：挂点 130（打击部位中间）→ 300（刀根，原版武器增益光效 8575 的 enchantStartDmyId 也用它）；火花效果 0.5 → 1.2 秒（效果一摘持续特效就断，火星要落 1 秒左右）
# 2026-11-04 开发工具实测（用户「挂在身上没问题 挂在护手上都不行」）：挂在武器上（effectType 1 / 2，挂点 10xxx / 20xxx）的光效行用普通效果挂时一律不显示（原版武器增益光效 303171 也不出），挂在身上的都正常 → 改挂人物模型的点位 1（跟着右手 R_Hand）/ 2（左手 L_Hand），原版身体光效也常用这两个，握武器的手 = 护手的位置；身体类型 0，演出类别 0
# 相杀的音效（用户 2026-11-04「相杀的音效还没做」）：光效行的持续音效（midstSeId，和持续特效火花同一格一起放；玩家脚本每挡下一次就清掉重挂 = 每次都响）。
# 第一版写的起手音效（那一格没有特效）一个都不响（用户「一个音效都没响」）：原版带音效的光效行都是同一格有特效，sfx 类音效、编号要在常驻的 cs_main 库里。
# -1 = 还没定（等用户在开发工具「相杀音效试听」里挑）
CLASH_SPARK_SE = -1
CUSTOM_VFX[VFX_CLASH_R] = (CLASH_SPARK_SFX, 1, 0, CLASH_SPARK_SE)
CUSTOM_VFX[VFX_CLASH_L] = (CLASH_SPARK_SFX, 2, 0, CLASH_SPARK_SE)
# 词条触发时一闪的光效（用户 2026-11-13「看看还有哪些词条适合加上特效」→「换气特效也自己做 其他的按照推荐方案就行」）：
# 都写 effectType 0（写了这项就不进演出类别，身上别的词条 / 增益光效在的时候不被压掉）；效果都很短（持续特效随效果结束收掉）或只有开头特效（原版就是一次性放的）
VFX_HUNT_BURST = (990070, 990071, 990072)   # 猎印被引爆 / 近身清除时的碎裂（消耗 1~3 层，sfx\hunt_burst.js），挂给那个敌人
VFX_BREATH = 990075                          # 换气：原版圣杯瓶回复光的精力绿版 + 精力光余韵（sfx\second_breath.js）
VFX_ALT_BOW, VFX_ALT_ITEM = 990080, 990081   # 交替：投掷 / 调香后（箭增伤）= 振奋香药的香雾 300541；射箭命中后（道具增伤）= 红羽七刃剑触发的红闪 301271
for _v in VFX_HUNT_BURST:
    CUSTOM_VFX[_v] = (_v, 220, 0, -1)
# 换气第三版（用户 2026-11-13「贴合原版风格设计一个特效」）：原版红露滴圣杯瓶回血光 301002 照红 → 蓝的规则换成精力绿（+ 腌制龟颈肉精力光的余韵），
# 照原版喝圣杯瓶放：开头特效格（一次放完、所有节点限时，不用收）、挂点 220（原版 5030 / 5352 挂这两个都用 220）。
# （第一版自己画的挂胸口、往镜头提前 3 米，看着很大；第二版挂腰间 160；用户都觉得不行）
CUSTOM_VFX[VFX_BREATH] = (VFX_BREATH, 220, 0, -1, 'init')
CUSTOM_VFX[VFX_ALT_BOW] = (300541, 905, 0, -1, 'init')       # 第 5 项 'init'：特效放开头特效那格（原版这两个都是开头特效，放完就完）
CUSTOM_VFX[VFX_ALT_ITEM] = (301271, 905, 0, -1, 'init')
VFX_FLASK = 5030         # 原版红露滴圣杯瓶给队友回血的光效行（持续 301002，演出类别 0）：回光每次回血时
VFX_HEAL_GOLD = 1642100  # 原版祷告「回复」的光效行（开头 4600 金色光，演出类别 0）：余裕致命一击后回复时


def hunt_burst_rows(v):
    """引爆 / 近身消耗猎印时挂给那个敌人的碎裂特效：消耗 1~3 层各一行（0.5 秒，持续特效随效果结束收掉；规则里 tfx，dark_core 按层数挑）"""
    return [dict(enemy=True, vfxId=b, effectEndurance=0.5) for b in VFX_HUNT_BURST]
# 法球射出去的辉石魔砾（迸发 / 星群，用户 2026-10-29：这次要真的发射）：照原版辉石魔砾（法术 4000）的子弹 10400000
# （飞行特效 523002、命中 523003、没锁定时也自动追踪），追踪加强（原版每秒 35 度）
PEBBLE_BULLET, PEBBLE_HOMING = 10400000, 120
# 用户 2026-11-12「法球发射时可以用毁灭流星的子弹吗」「出手就直接冲」：子弹行照毁灭流星（法术 4220）单颗星的第二段 10422030
# （飞行特效 523907、命中 523908，40 → 45 米/秒、0.5 秒），不要第一段的盘旋；攻击行照旧用魔砾的 40000（固定魔力）。
# 原版这一段只在锁定时追踪、每秒 45 度：改成没锁定也自动找目标（同魔砾），追踪每秒 250 度（速度快一倍，转弯半径约 9.5 米，和以前的魔砾差不多）
STAR_BULLET, STAR_HOMING = 10422030, 250
ORB_POISE = 5.0                        # 法球每颗的削韧：固定 5 点，不随伤害档变（用户 2026-11-13；以前 3 + 伤害 / 60）
# 同一天「然后法球的子弹改为先盘旋吧」：前面接上毁灭流星的第一段 10422015（0.3 秒里从 12 米/秒慢下来、往两边散开最多 60 度、
# 每秒转 620 度对准目标，时间到放出第二段冲出去：HitBulletID + launchConditionType 4），和原版一样；两段都用同一行攻击
STAR_BULLET1 = 10422015
ICON_WEAKEN = 20468    # 同上的图标（菱形框、剑、向下箭头 = 攻击力降低）
ICON_CLAW = 20254      # 钩爪护符（跳跃攻击提升，效果 321800）的图标：天堑的跳劈加成
# 用户 2026-10-28：有原版对应的照原版补图标 / 光效，没有的找最接近的（各自从哪借的见 光效对照.md）
VFX_THIRST = 321600    # 鲜血君王的欢愉（周围出血后 20 秒，效果 321601）：渴血的 30 秒状态，和它同一个触发
VFX_POISE = 5805       # 岩棘裂纹露滴（削韧提升，效果 511026，图标 20465）：精准格挡、鸡尾酒的削韧
VFX_ELEGANCE = 5307    # 振奋香（攻击力提升，效果 503501）：优雅
VFX_BLOODRAGE = 5280   # 狂热香药（加攻、加精力、受伤增加，效果 503550）：血狂
VFX_DESPERATE = 8660   # 战吼（战灰，攻击力提升，效果 1811）：背水
VFX_CHARGE = 312500    # 米莉森的义手（连续攻击时提升，效果 312505）：积蓄
VFX_AERIAL = 5846      # 风之结晶露滴（效果 511015）：凌空可以再跳时
ICON_HASTE = 20420       # 风之结晶露滴（效果 511015）的状态图标：动作加快（处刑、转念）
COCKTAIL_VFX = {'atk': 8725, 'cut': 5370, 'stam': 5352, 'hp': 5848, 'fp': 5842, 'poise': VFX_POISE}   # 水母盾、煮螃蟹、腌龟颈肉、赤红爆裂结晶露滴、星光碎片、岩棘
ICON_WARD = 20408      # 珍珠泡状露滴（效果 3507，光效就是 VFX_WARD）的图标：无敌期间
BUBBLE_TEAR = 3507     # 珍珠泡状露滴的效果（八种减伤 ×0.1、受到伤害就消失 deleteCriteriaDamage、泡泡光效 5866）：泡盾照抄
# 自制状态图标（buff 图标）：放在原版图集 SB_Status_00（1024×512）右边的空白里（原版图标只到 x=612；尺寸不改，原版图标一点不动；
# launcher\reroll\menu.py 启动时从本机 Data0 取原版合进去）。编号 20600 起（原版 20005 ~ 20525，GoldenAge 用到 20752，都在 20xxx）。
# 第一版扩了 SB_Status_01_dlc 的尺寸、编号用 21000 起，游戏里壁垒的图标没正常显示（用户 2026-10-28），改成照原版 / GoldenAge 能用的条件。
# 画法见 make_icons.py：原版方框 + 护符画（或借的原版道具图）+ 升降箭头；叠层的每层一个、左上角写层数（用户 2026-10-28，照 GoldenAge「多才多艺」）。
# (名字, 原版道具图标编号, 箭头, 层数（0 = 不写数字）)；每个名字占 20 个编号（第 k 个从 20600 + 20k 起），只往后加，编号就不会变
STATUS_ICONS = [('flow', 18890, 'up', 5), ('bulwark', 18490, 'up', 6), ('orb', 18010, 'up', 7), ('desperate', 18270, 'up', 9),
                ('aerial', 18800, 'up', 5), ('bloodrage', 225, 'up', 10), ('elegance', 631, 'up', 10), ('charge', 18210, 'up', 0),
                ('momentum', 18871, 'up', 3),           # 乘势：借亚历山大的碎片的护符画
                ('stillness', 3228, 'down', 5),         # 咒痕「沉寂」1~5 层（造成的伤害降低）：借「死亡的咒痕」的道具画（用户 2026-11-04「补一下沉寂的buff图标」）；
                                                        # rr/curses.py 的 ECHO_ICON0 = 20600 + 20 × 9 要和这里的顺序对上
                ('corrosion', 18550, 'down', 5),        # 咒痕「蚀甲」1~5 层（受到的伤害增加）：借熔炉鳞护符的画（壁垒已经用了龙徽盾）（用户 2026-11-04「蚀甲也要把图标加上」）；
                                                        # rr/curses.py 的 ERODE_ICON0 = 20600 + 20 × 10
                ('gift', 18750, 'up', 0),               # 对勇于挑战者的馈赠（战斗复活还能用时显示）：借替身枝护符的画；rr/curses.py 的 GIFT_ICON = 20600 + 20 × 11
                ('myriad', 18360, 'down', 4),           # 咒痕「千变」诅咒 1~4 档（造成的伤害降低）：借双刃护符的画；rr/curses.py 的 VARY_ICON_C0 = 20600 + 20 × 12
                ('myriad_up', 18360, 'up', 5)]          # 千变祝福 1~5 档（造成的伤害提升）；VARY_ICON_B0 = 20600 + 20 × 13
STATUS_ICON_BASE = 20600
AERIAL_CHAIN = 5           # 凌空：连跳最多几层


def status_icon(name, n=0):
    """自制状态图标的编号：name 的第 n 层（不叠层的 n = 0）"""
    k = next(i for i, x in enumerate(STATUS_ICONS) if x[0] == name)
    if n > STATUS_ICONS[k][3]:
        raise ValueError('状态图标 %s 只画到 %d 层' % (name, STATUS_ICONS[k][3]))
    return STATUS_ICON_BASE + 20 * k + max(n - 1, 0)
# 效果的状态（stateInfo）。152：命中时给对方挂 atkOccurrenceSpEffectId 的开关（原版带这个字段的 55 行有 54 行是 152：油脂、武器附魔、
# 蛇弓的常驻；不是 152 字段就不起作用，2026-10-28 查反扑没生效查到：命中事件 land、混沌原来都是 0）。
# 449：玛莲妮亚的大卢恩的虚血（原版效果 650 只带这一项；VINS、GoldenAge 的虚血也是挂一个 449 的效果，夺回量用 regainRate 调）
ATK_OCCUR_STATE, REGAIN_STATE = 152, 449
# 连击值（打中时扫全场用）照原版「夺蓝破露滴」（攻击命中回专注）：累积行 20511031（stateInfo 307 = 攻击命中计数，阈值 1，同一状态号共用一个计数），
# 到了挂 20511032（计数清零、再循环挂效果）。词条库的命中触发用 303~306（按稀有度），这里用 307：和夺蓝破露滴同一个计数，两边都是阈值 1，互不影响
HIT_ACC = (20511031, 20511032)
# 停用的新词条（不再掉落，效果行、玩家脚本照旧，已有的装备照样生效）：霸王（一次性霸体太俗套，用户 2026-11-04 换成乘势）
# 用户 2026-11-05「把霸王这个词条加回来 然后重蓄霸体的词条改为减少重蓄所需时间」：霸王重新掉落（沉稳不再给蓄力霸体，霸体只剩战技这一处，不再俗套）
# 用户 2026-11-06「把悲伤 老练骑手 臭气嘲讽 致命长矛 远射 disable 了」（实战很少能触发或用处很小）、「永续记忆可以删了」、「心流这词条去掉吧」：不再掉落，已有的装备照旧
RETIRED_KEYS = ('raise_attack_power_by_pct_for_seconds_when_summo', 'increase_damage_of_attacks_while_on_horseback_by',
                'your_crouching_release_stinky_lightly_poisonous', 'increase_thrusting_weapons_counterattack_damage',
                'increase_max_range_of_bows_and_crossbows_by_poin', 'increase_memory_slots_by', 'dr_flow')
UNSHAKEN_GUARD = 0.25  # 不动如山：防御时掉的精力只剩 1/4（效果的 guardStaminaMult）
AIL_RANDOM = ['poison', 'rot', 'bleed', 'frost']                         # 随机附加的异常（几乎所有敌人都会中的四种）
REGIST = ('registPoizonChangeRate', 'registDiseaseChangeRate', 'registBloodChangeRate', 'registCurseChangeRate', 'registFreezeChangeRate',
          'registSleepChangeRate', 'registMadnessChangeRate')                    # 自身异常抗性的倍率（连锁的代价）
NEUTRAL_FIELDS = dict(iconId=-1, fallDamageRate=1.0, spCategory=0)   # 照 360400（长尾猫）复制时去掉它自己的效果
# 低血流派的血量分段（用户 2026-11-11「低于20%血量触发的词条调为低于40%触发」）：(失去几成, 血量低于 %, 血量不低于 %；0 = 不限)，
# 每失去 10％ 一档、从满血算起：血量 30~40％ = 失去 6 成 … 10％ 以下 = 9 成（绝境 / 残躯 / 搏命）
LOW_HP_BANDS = ((6, 40, 30), (7, 30, 20), (8, 20, 10), (9, 10, 0))
RULE_AFFIXES = [
    # 姿态舞（bd设计.md 3.1）：单手冷静、双手愤怒，切换握法本身是资源
    dict(key='dr_calm', suffix=['Calm', '静心'], slots=('head', 'talisman'), icon=18160, vals=((4, 3), (6, 4), (8, 5), (10, 6)),
         zh='单手持握武器时，能提升精力的恢复速度（＋%s），并减少%s％受到的伤害',
         en='While wielding with one hand, raises stamina recovery (+%s) and reduces damage taken by %s%%',
         rows={'calm': lambda v: dict(iconId=ICON_STAM, vfxId=VFX_CALM, effectEndurance=-1.0, staminaRecoverChangeSpeed=v[0],
                                      **{x: 1 - v[1] / 100 for x in CUT_FIELDS})},
         logic=[dict(type='while', cond='grip1', effect='calm')]),
    # 怒意：(攻击力 +%, 受到的伤害 +%)；用户 2026-10-30「加强怒意的伤害加成 整体增加5%」：攻击力每档 +5（以前和受伤一样 8 / 10 / 12 / 15）；
    # 2026-11-06「怒意伤害提升到25% 然后说明对剑类武器也生效」（对剑这一类武器，用户认为巨力握持对它们不生效；2026-11-11 用户：说明里改叫「成对武器」）：攻击力每档再 +5（规则只看是不是双手持握，本来就不挑武器）
    dict(key='dr_rage', suffix=['Rage', '怒意'], slots=('body', 'talisman'), icon=18919, vals=((18, 8), (20, 10), (22, 12), (25, 15)),
         zh='以双手共持武器时（成对武器同样生效），能提升%s％攻击力，但受到的伤害也会增加%s％',
         en='While two-handing (paired weapons included), raises attack power by %s%%, but damage taken also rises by %s%%',
         rows={'rage': lambda v: dict(iconId=ICON_ATK, vfxId=VFX_RAGE, effectEndurance=-1.0,
                                      **{x: 1 + v[0] / 100 for x in ATK_RATES}, **{x: 1 + v[1] / 100 for x in DEF_RATES})},
         logic=[dict(type='while', cond='grip2', effect='rage')]),
    # 转念（用户 2026-11-06「切换恢复专注值改为切换握持姿势后的下一击动作最高加快40%」）：切换后一直等到下一次攻击，这一击的动作（直到动作换掉）加快
    dict(key='dr_turn', suffix=['Turnabout', '转念'], slots=('body',), vals=(10, 20, 30, 40),
         zh='切换持握方式后的下一次攻击，动作能加快%s％',
         en='After switching grips, your next attack plays %s%% faster',
         rows={'ready': lambda v: dict(iconId=ICON_HASTE, vfxId=VFX_AERIAL, effectEndurance=-1.0)},
         logic=[dict(type='stack', add=['style'], consume=['attack'], max=1, act_speed=lambda v: v, fx='ready')]),
    dict(key='dr_break', suffix=['Breakthrough', '破势'], slots=('talisman',), icon=18936, vals=((15, 20), (20, 30), (25, 40), (35, 50)),
         zh='切换持握方式后的3秒内，下一次攻击能提升%s％攻击力与%s％削韧',
         en='For 3 seconds after switching grips, your next attack deals +%s%% damage and +%s%% poise damage',
         rows={'break': lambda v: dict(iconId=ICON_ATK, vfxId=VFX_BREAK, effectEndurance=-1.0, saAttackPowerRate=1 + v[1] / 100,
                                       **{x: 1 + v[0] / 100 for x in ATK_RATES})},
         logic=[dict(type='stack', add=['style'], consume=['attack'], max=1, timeout=3.0, fx='break')]),
    # 换气（用户 2026-11-09「新增一个护符限定词条 双手共持武器时切换武器回复所有精力 随稀有度减少冷却从20s到10s」）：玩家脚本 ExecWeaponChange 发 wswitch2；stamina 给得足够大，dark_core 按精力上限截断
    dict(key='dr_second_breath', suffix=['Second Breath', '换气'], slots=('talisman',), icon=18160, vals=(20, 17, 13, 10),
         zh='以双手共持武器时，切换武器能恢复全部精力（冷却%s秒）',
         en='While two-handing, switching weapons fully restores stamina (%ss cooldown)',
         # 用户 2026-11-13「换气特效也自己做」→「贴合原版风格」：回满时放精力绿的圣杯瓶回复光（特效 990075 在开头特效格，一次放完，效果行多长都不影响）
         rows={'flash': lambda v: dict(vfxId=VFX_BREATH, effectEndurance=0.5)},
         logic=[dict(type='on', event='wswitch2', cooldown=lambda v: v, stamina=lambda v: 9999, effect='flash')]),
    dict(key='dr_flow', suffix=['Flow', '心流'], slots=('talisman',), icon=18890, vals=(5, 5, 5, 5), rarities=('Legendary',), keystone=True,
         zh='每次切换持握方式都能积蓄一层心流，每层提升%s％攻击力，最多5层；受到攻击时心流会散去，且每次切换都会消耗15点精力',
         en='Each grip switch builds a stack of Flow, raising attack power by %s%% per stack (up to 5); '
            'Flow fades when you are hit, and each switch costs 15 stamina',
         rows={'flow': lambda v: [dict(iconId=status_icon('flow', n), vfxId=VFX_FLOW, effectEndurance=-1.0, **{x: 1 + n * v / 100 for x in ATK_RATES})
                                  for n in range(1, 6)]},
         logic=[dict(type='stack', add=['style'], clear=['hit'], max=5, timeout=10.0, fx='flow', stamina=lambda v: -15, log='心流')]),
    # 壁垒防反（bd设计.md 3.2）：防住的攻击存成壁垒，防御反击一次放出；冲击波用碾压的档位（按右手武器攻击力）
    # 每层自己减伤（用户 2026-10-28：壁垒本身也要有效果，不能只靠反震放出去）；第 n 层那一行减 n × v[1]％（同静心的减伤字段）
    dict(key='dr_bulwark', suffix=['Bulwark', '壁垒'], slots=('body', 'talisman'), icon=18490, vals=((3, 2), (4, 2.5), (5, 3), (6, 3)),
         zh='防住攻击时能积蓄一层壁垒，最多%s层，每层减少%s％受到的伤害；10秒不防御就会散去',
         en='Blocking an attack builds a layer of Bulwark (up to %s), each reducing damage taken by %s%%; it fades after 10 seconds without blocking',
         rows={'layer': lambda v: [dict(iconId=status_icon('bulwark', n), vfxId=VFX_BULWARK, effectEndurance=-1.0,
                                        **{x: 1 - n * v[1] / 100 for x in CUT_FIELDS}) for n in range(1, v[0] + 1)]},
         logic=[dict(type='stack', add=['guard_hit'], max=lambda v: v[0], timeout=10.0, fx='layer', log='壁垒')]),
    dict(key='dr_recoil', suffix=['Recoil', '反震'], slots=('leg', 'talisman'), icon=18820, vals=(10, 14, 18, 25),
         zh='防御反击时能释放全部壁垒，每层追加右手武器攻击力%s％的冲击，并削减强韧度',
         en='A guard counter releases all Bulwark, each layer adding a shockwave of %s%% of your right-hand weapon attack that breaks poise',
         logic=[dict(type='on', event='guard_counter', take='dr_bulwark', burst=lambda v: v)]),   # 冲击波子弹自带原版特效（SHOCK_SFX）
    # 用户 2026-10-28 改：防御照常扣精力，精力不够时不破防，不够的部分扣两倍的血。
    # 标记行的 guardStaminaMult（防御时掉的精力倍率，原版大盾护符 0.8、完美防御 0.35）让引擎只扣 1/4，基本不会自己判破防；
    # dark_core 按「掉的 × 4」算真正的消耗、补扣剩下的，不够的扣血；精力见底时引擎量不出来，用最近一次量到的估。
    # 玩家脚本把破防（包括吹飞 / 击飞倒地的）换成防御挨重击兜底
    dict(key='dr_unshaken', suffix=['Unshaken', '不动如山'], slots=('talisman',), icon=18220, vals=(200, 200, 200, 200), rarities=('Legendary',),
         keystone=True, zh='举盾防御时不会被破防，但精力不足时，不足的部分会改为扣除血量（不足精力量的%s％）',
         en='Your guard can no longer be broken; when you lack the stamina to block, the shortfall is taken from your HP instead (%s%% of it)',
         mark=dict(guardStaminaMult=UNSHAKEN_GUARD),
         logic=[dict(type='on', event=ev, guard_scale=1 / UNSHAKEN_GUARD, short_hp=lambda v: v) for ev in ('guard_hit', 'guard_light')]),
    dict(key='dr_poise_guard', suffix=['Precise Guard', '精准格挡'], slots=('weapon',), vals=(50, 50, 50, 50), rarities=('Legendary',),
         zh='用这把武器防住攻击后，3秒内的下一次攻击能提升%s％削韧',
         en='After blocking an attack with this weapon, your next attack within 3 seconds deals +%s%% poise damage',
         rows={'buff': lambda v: dict(iconId=ICON_POISE, vfxId=VFX_POISE, effectEndurance=-1.0, saAttackPowerRate=1 + v / 100)},
         logic=[dict(type='stack', add=['guard_hit'], consume=['attack'], max=1, timeout=3.0, fx='buff')]),
    # 血魔法（bd设计.md 3.5）：专注换成血量，低血量更强，打倒敌人回血
    # 用户 2026-11-19「血契传说挡位消耗血量改为专注值的300% 其余缩放」：150 → 300；随后「算了改成500吧 说明也改一下 不用百分比了 用5倍」：
    # 500（只出传说，其他档同比例也是 500），说明写「专注值消耗量的5倍」（text_val：说明里的数字 = v / 100，规则行仍是 fp_to_hp=500）。
    # 用户 2026-11-20「血契数值调整为4倍」：400
    dict(key='dr_blood_pact', suffix=['Blood Pact', '血契'], slots=('talisman',), icon=18390, vals=(400, 400, 400, 400), rarities=('Legendary',),
         # 同日「血不够就扣死」：扣血不再保底留 1（rules.rs fp_to_hp 用 HpCost），说明照燃血补一句会死
         keystone=True, zh='使用魔法、祷告与战技时，不再消耗专注值，改为消耗血量（专注值消耗量的%s倍），血量耗尽时会死亡',
         en='Sorceries, incantations and skills no longer cost FP; they cost HP instead (%s× the FP cost). Running out of HP kills you', text_val=lambda v: v / 100,
         logic=[dict(type='while', fp_to_hp=lambda v: v)]),
    # 用户 2026-11-03：传说提升到 50％（以前 6 / 9 / 12 / 16），去掉战技那一份，只管魔法、祷告；
    # 2026-11-18「血涌的触发条件改到25%血以下」（以前血量低于一半）
    dict(key='dr_blood_surge', suffix=['Bloodsurge', '血涌'], slots=('head',), vals=(20, 30, 40, 50),
         zh='血量低于25％时，能提升%s％魔法与祷告的攻击力',
         en='While HP is below 25%%, raises the attack of sorceries and incantations by %s%%',
         rows={k: (lambda src, icon: lambda v: dict(src=src, iconId=icon, vfxId=VFX_SURGE if icon > 0 else -1, effectEndurance=-1.0,
                                                    **{f: 1 + v / 100 for f in SPELL_RATES}))(src, icon)
               for k, src, icon in (('sorcery', 330010, 20006), ('incant', 330500, -1))},
         logic=[dict(type='while', hp_below=25, effect=k) for k in ('sorcery', 'incant')]),
    dict(key='dr_blood_drink', suffix=['Blood Drinker', '饮血'], slots=('arm', 'talisman'), icon=18640,
         vals=((2, 20), (3, 30), (4, 40), (6, 60)),
         zh='打倒敌人时，能恢复血量（最大血量的%s％＋%s点）', en='Defeating an enemy restores HP (%s%% of max HP + %s)',
         rows={'heal': lambda v: dict(src=350301, changeHpRate=-float(v[0]), changeHpPoint=-v[1])},
         mark=dict(stateInfo=199, applyIdOnGetSoul='heal')),
    # 反击回血（bd设计.md 3.3）：受伤后往前压，命中把失去的血夺回来。用原版玛莲妮亚的大卢恩的虚血：标记带 stateInfo 449，
    # 窗口（约 5 秒）、每一下夺回多少（看武器和招式，攻击表 wepRegainHpScale，跳劈 / 蓄力多）都是原版的，稀有度只用 regainRate 拉开。
    # 用户 2026-10-28：自己算窗口和比例的那版（积木 rally 规则）没生效，改参考 VINS / GoldenAge 的虚血
    # 夺回量按稀有度是大卢恩的 20 ~ 50％（最高 50％），说明里不写这个比例（用户 2026-10-28）；数值 ×2 → 40 ~ 100％（用户 2026-11-01）
    dict(key='dr_rally', suffix=['Rally', '反扑'], slots=('body', 'talisman'), icon=18630, vals=(40, 60, 80, 100),
         zh='受到伤害的当下，能透过攻击夺回部分失去的血量',
         en='Right after taking damage, attacks regain part of the lost HP',
         mark=dict(stateInfo=REGAIN_STATE, regainRate=lambda v: v / 100.0)),
    # 用户 2026-11-03：传说 0 血提升 60％（每失去 10％ 血量 +6％，按层直接加；以前 1 / 1.5 / 2 / 3、按层复利）
    dict(key='dr_desperate', suffix=['Desperation', '背水'], slots=('head', 'talisman'), icon=18270, vals=(2, 3, 4, 6),
         # 用户 2026-11-03：说明写明是全属性伤害（行改的是 atkPlayer / EnemyDmgCorrectRate 的五种伤害，物理、魔力、火、雷、圣都算，法术也吃）
         zh='失去的血量越多，造成的全属性伤害越高（每失去10％血量，全属性伤害提升%s％）',
         en='The more HP you have lost, the more damage of all types you deal (+%s%% per 10%% of HP lost)',
         # 第 n 档（失去 10n％ ~ 10(n+1)％ 血量）只挂第 n 行，带写着 n 的图标；倍率 1 + n × v％
         # （用户 2026-10-28：叠层的图标要显示层数，原来 9 行同时挂、只有第一行带图标）
         rows={'lv%d' % n: (lambda n: lambda v: dict(iconId=status_icon('desperate', n), vfxId=VFX_DESPERATE, effectEndurance=-1.0,
                                                     **{f: 1 + n * v / 100 for f in ATK_RATES}))(n) for n in range(1, 10)},
         logic=[dict(type='while', hp_below=100 - 10 * n, effect='lv%d' % n, **({'hp_above': 90 - 10 * n} if n < 9 else {}))
                for n in range(1, 10)]),
    # ---- 低血流派（用户 2026-11-10 给的表，数值按讨论改过：按损失血量的百分比算，不按点数；传说护符去掉；一管血 = 3 管精力（2026-11-11 从 5 管削弱）；回光每刀都回、挥空也扣血）
    # 燃血：攻击（attack 脉冲 = 出招，skill = 战技状态）掉的精力由 dark_core 补回来、改扣血（可以扣到死）
    # 2026-11-11 用户「近战也不生效」：attack 脉冲只亮 0.1 秒（出招那一下），游戏扣攻击的精力在挥出去、攻击判定出来时，晚零点几秒，
    # 那时脉冲已经灭了 → 再看 swing（普通攻击的状态里每帧发：轻 / 重 / 冲刺 / 翻滚、后撤攻击 / 跳跃 / 防反）；刚翻滚的不算（攻击刚结束接翻滚）
    # 2026-11-11 用户「燃血削弱一下 满血等于三个精力条」（以前一管精力 = 最大血量的 20％，满血 = 5 管）：v = 满血折几管精力，规则按一管精力扣 100 / v ％
    dict(key='dr_bloodburn', suffix=['Bloodburn', '燃血'], slots=('talisman',), icon=18930, vals=(3, 3, 3, 3), rarities=('Legendary',),
         keystone=True, zh='攻击不再消耗精力，改为消耗血量，血量耗尽时会死亡',   # 用户 2026-11-11：说明不写几管精力折一管血
         en='Attacks no longer cost stamina; they cost HP instead. Running out of HP kills you',
         logic=[dict(type='while', cond='attack', stamina_to_hp=lambda v: round(100 / v, 2)),
                dict(type='while', cond='swing', unless='roll', stamina_to_hp=lambda v: round(100 / v, 2)),
                dict(type='while', cond='skill', stamina_to_hp=lambda v: round(100 / v, 2))]),
    # 绝境：血量低于 20％ 时轻攻击增伤，每失去 10％ 血量 v％（低于 20％ = 失去 8 成，低于 10％ = 9 成）；轻攻击 = 相杀那套 clash_l_r / clash_l_l
    dict(key='dr_last_stand', suffix=['Last Stand', '绝境'], slots=('head', 'talisman'), icon=18929, vals=(4, 6, 8, 10),
         zh='血量低于40％时，每失去10％血量，能提升%s％轻攻击的伤害',
         en='Below 40%% HP, light attacks deal +%s%% damage per 10%% of HP lost',
         rows={'lost%d' % n: (lambda n: lambda v: dict(iconId=ICON_ATK, vfxId=VFX_SURGE, effectEndurance=-1.0,
                                                         **{f: 1 + n * v / 100 for f in ATK_RATES}))(n) for n, _b, _a in LOW_HP_BANDS},
         logic=[dict(type='while', cond=c, hp_below=b, effect='lost%d' % n, **({'hp_above': a} if a else {}))
                for c in ('clash_l_r', 'clash_l_l') for n, b, a in LOW_HP_BANDS]),
    # 残躯：血量低于 40％ 时减伤，每失去 10％ 血量 v％（传说 9 成 = 45％）；
    # 用户 2026-11-13「残血加减伤那个词条数值提升到5%」：1.5 / 2 / 2.5 / 3 → 2.5 / 3.5 / 4 / 5（传说 5，其他档同比例取到 0.5）
    dict(key='dr_tattered', suffix=['Tattered', '残躯'], slots=('body', 'talisman'), icon=18540, vals=(2.5, 3.5, 4, 5),
         zh='血量低于40％时，每失去10％血量，受到的伤害减少%s％',
         en='Below 40%% HP, you take %s%% less damage per 10%% of HP lost',
         rows={'lost%d' % n: (lambda n: lambda v: dict(iconId=20408, vfxId=VFX_BULWARK, effectEndurance=-1.0,
                                                         **{f: 1 - n * v / 100 for f in CUT_FIELDS}))(n) for n, _b, _a in LOW_HP_BANDS},
         logic=[dict(type='while', hp_below=b, effect='lost%d' % n, **({'hp_above': a} if a else {})) for n, b, a in LOW_HP_BANDS]),
    # 搏命：血量低于 40％ 时轻攻击的削韧（saAttackPowerRate），每失去 10％ 血量 v％
    dict(key='dr_desperado', suffix=['Desperado', '搏命'], slots=('arm', 'talisman'), icon=18924, vals=(4, 6, 8, 10),
         zh='血量低于40％时，每失去10％血量，能提升%s％轻攻击的削韧',
         en='Below 40%% HP, light attacks deal +%s%% poise damage per 10%% of HP lost',
         rows={'lost%d' % n: (lambda n: lambda v: dict(iconId=ICON_POISE, vfxId=VFX_POISE, effectEndurance=-1.0,
                                                         saAttackPowerRate=1 + n * v / 100))(n) for n, _b, _a in LOW_HP_BANDS},
         logic=[dict(type='while', cond=c, hp_below=b, effect='lost%d' % n, **({'hp_above': a} if a else {}))
                for c in ('clash_l_r', 'clash_l_l') for n, b, a in LOW_HP_BANDS]),
    # 回光：血量低于 20％ 时，近战每次命中回最大血量的 v％（每刀都回，回到 20％ 以上就停；用户 2026-11-11 从 10％ 改成 20％）
    dict(key='dr_last_light', suffix=['Last Light', '回光'], slots=('leg', 'talisman'), icon=18650, vals=(2, 3, 4, 5),
         zh='血量低于20％时，近战攻击每次命中都能恢复%s％最大血量',
         en='Below 20%% HP, every melee hit restores %s%% of max HP',
         # 用户 2026-11-13（特效提案）：每次回血闪一下红露滴圣杯瓶的回血光（0.4 秒，每刀都会触发，所以短）
         rows={'flash': lambda v: dict(vfxId=VFX_FLASK, effectEndurance=0.4)},
         logic=[dict(type='on', event='land', unless=['shot_hit', 'item_hit', 'cast_hit', 'cast'], hp_below=20, hp_pct=lambda v: v, effect='flash')]),
    # 血瓮：血量上限加固定点数；dark_core 按基础血量上限挑最接近的「血量上限 +k％」档（custom_shared 的 hpmax，1~HPMAX_TIERS％）
    dict(key='dr_blood_urn', suffix=['Blood Urn', '血瓮'], slots=('talisman',), icon=18600, vals=(60, 120, 180, 250),
         zh='血量上限＋%s点', en='Max HP +%s',
         logic=[dict(type='while', hp_flat=lambda v: v)]),
    dict(key='dr_blood_debt', suffix=['Blood Debt', '血债'], slots=('talisman',), icon=18610, vals=(50, 50, 50, 50), rarities=('Legendary',),
         # 用户 2026-11-06 去掉代价（以前红露滴不再回血：on flask deny_heal=3），喝红露滴时回专注；
         # 2026-11-13「血债的增加蓝瓶恢复删掉 改为减少红瓶50%回复」：回专注去掉，代价改成红露滴恢复量 −v％（原版字段，照「血热」的写法）
         keystone=True, zh='反扑夺回的血量变为3倍，但红露滴圣杯瓶的恢复量减少%s％',
         en='Rally regains three times as much HP, but Flasks of Crimson Tears restore %s%% less HP',
         mark=dict(regainRate=3.0,     # 虚血倍率和反扑的乘起来（GoldenAge 也是另挂 regainRate 2 / 0.5 的效果加减虚血）
                   changeHpEstusFlaskCorrectRate=lambda v: 1 - v / 100),
         logic=[]),
    # 武器特效能出的稀有度：用户 2026-10-30「普通也放开更多特效」→ 渴血、碎冰、巨人杀手普通起都能出（以前稀有 / 稀有 / 史诗起），传说专属的照旧
    dict(key='dr_thirst', suffix=['Bloodthirst', '渴血'], slots=('weapon',), vals=(3, 3, 3, 3), rarities=RAR,
         zh='周围发生出血后的30秒内，攻击命中敌人能恢复%s％血量',
         en='For 30 seconds after blood loss occurs nearby, hitting an enemy restores %s%% HP',
         rows={'state': lambda v: dict(iconId=ICON_BLEED, vfxId=VFX_THIRST, effectEndurance=30.0), 'flash': lambda v: dict(vfxId=VFX_HEAL, effectEndurance=0.5)},
         mark=dict(spCategory=20, invocationConditionsStateChange1=379, cycleOccurrenceSpEffectId='state'), events={'bleed_near': 'state'},
         logic=[dict(type='on', event='land', cond='bleed_near', hp_pct=lambda v: v, effect='flash')]),
    # 强韧：霸体照霸王 / 浮空，交给玩家脚本（DR_DamageLevel：这两行在身上时，会硬直的受击都按无硬直，不是一次性的）。
    # 原来整行复制忍耐 1650、靠它的受击等级替换，和霸王 v2 一样不起作用（用户 2026-10-28：喝药时和之后依然没有霸体）；
    # 减伤也没去掉（改的是 CUT_FIELDS，忍耐的减伤在 DEF_RATES）。现在两行只有忍耐的图标、光效
    dict(key='dr_flask_endure', suffix=['Steadfast Draught', '强韧'], slots=('leg', 'talisman'), icon=18903, vals=(1, 1.5, 2, 3),   # 用户 2026-11-09 减半（原来 2 / 3 / 4 / 6）
         zh='饮用红露滴圣杯瓶时，以及饮用后的%s秒内，受到攻击不会被打断',
         en='While drinking a Flask of Crimson Tears and for %s seconds after, attacks do not interrupt you',
         rows={'hold': lambda v: dict(iconId=20430, vfxId=8570, effectEndurance=-1.0),
               'after': lambda v: dict(iconId=20430, vfxId=8570, effectEndurance=float(v))},
         hks={'endure_hold': 'hold', 'endure_after': 'after'},
         logic=[dict(type='while', cond='flask', effect='hold'), dict(type='on', event='flask', when='end', effect='after')]),
    dict(key='dr_physick_ward', suffix=['Physick Ward', '无敌'], slots=('talisman',), icon=18916, vals=(2, 3, 4, 6),   # 用户 2026-11-06 最高加到 6 秒（原来 1 / 1.5 / 2 / 3）；2026-11-08 喝药动作中无敌，喝完后再计时（同忍耐的 hold + after）
         zh='饮用灵药圣杯瓶时处于无敌状态，喝完后的%s秒内仍保持无敌',
         en='You are invincible while drinking the Flask of Wondrous Physick and for %s seconds afterward',
         rows={'ward': lambda v: dict(iconId=ICON_WARD, vfxId=VFX_WARD, effectEndurance=float(v)),
               'hold': lambda v: dict(iconId=ICON_WARD, vfxId=VFX_WARD, effectEndurance=-1.0)}, iframes=['ward', 'hold'],
         logic=[dict(type='while', cond='physick', effect='hold'), dict(type='on', event='physick', when='end', effect='ward')]),
    # 泡盾（用户 2026-11-01）：传说专属，持续 15 秒没受到伤害就挂上原版珍珠泡状露滴的泡泡（BUBBLE_TEAR：八种减伤 ×0.1、受伤就消失），
    # 下一次受到的伤害减少 90%。dark_core 的 quiet 规则（rules.rs Kind::Quiet）：受击 / 泡泡被打掉都重新计时，刚戴上也从头计时
    dict(key='dr_bubble', suffix=['Bubble Ward', '泡盾'], slots=('head', 'talisman'), icon=18925, vals=(15, 15, 15, 15), rarities=('Legendary',),
         zh='持续%s秒未受到伤害时，会生成泡状护盾，使下一次受到的伤害减少90％',
         en='After %s seconds without taking damage, a bubble shield forms that reduces the next damage you take by 90%%',
         # direct：这份底子缺「挂给自己」位，dark_core 挂不上（2026-11-09 玩家报泡盾没生效）
         rows={'shield': lambda v: dict(src=BUBBLE_TEAR, direct=True, iconId=ICON_WARD, effectEndurance=-1.0, isContractSpEffectLife=0)},
         logic=[dict(type='quiet', secs=lambda v: v, effect='shield')]),
    # 环绕法球（bd设计.md 3.4）：施法攒法球，后撤 / 蹲下射出去
    # 法球自己的效果（用户 2026-10-28：法球本身也要有用，具体让我定）：每颗减少魔法、祷告消耗的专注值（施法攒球、球让施法更省；
    # 也抵掉一部分星群的代价）；第 n 颗那一行减 n × v[1]％
    dict(key='dr_orb', suffix=['Gathering', '凝聚'], slots=('head', 'talisman'), icon=18010, vals=((1, 5), (2, 5), (2, 7), (3, 7)),
         zh='施放魔法或祷告后，能生成一颗法球环绕身边，最多%s颗，每颗减少%s％魔法、祷告消耗的专注值；20秒不施法就会散去',
         en='Casting a sorcery or incantation creates an orb that circles you (up to %s), each reducing the FP cost of sorceries and incantations '
            'by %s%%; they fade after 20 seconds without casting',
         rows={'orb': lambda v: [dict(iconId=status_icon('orb', n), vfxId=VFX_ORBIT[n - 1], effectEndurance=-1.0,
                                      **{k: 1 - n * v[1] / 100 for k in ('magicConsumptionRate', 'miracleConsumptionRate', 'shamanConsumptionRate')})
                                 for n in range(1, v[0] + 1 + max(HAWK_ORBS))]},   # 多的给盈满（按它的稀有度多 1~4 颗）
         # cast_new：新的一次施法（玩家脚本发，连续施法的第 2 / 3 下也算；cast 是「施法中」的持续状态，2026-10-29 改）
         logic=[dict(type='stack', add=['cast_new'], max=lambda v: v[0], max_boost_key='dr_hawk', max_boost=list(HAWK_ORBS), timeout=20.0, fx='orb', log='法球')]),
    # 用户 2026-11-03：每颗法球提升到 20％（以前 2 / 3 / 4 / 5）；2026-11-04「降低加成吧 降低到每颗15%」（法球最多 7 颗后太强）→ 6 / 9 / 12 / 15
    dict(key='dr_orb_flow', suffix=['Orbflow', '涌流'], slots=('body',), vals=(6, 9, 12, 15),
         zh='环绕身边的法球越多，魔法、祷告的攻击力越高（每颗提升%s％）',
         en='The more orbs circle you, the higher the attack of your sorceries and incantations (+%s%% per orb)',
         # 有 n 颗时第 1~n 行同时挂、倍率相乘：第 n 行取 (1 + n·v) / (1 + (n-1)·v)，乘起来正好 1 + n·v（每颗 +v，不复利；2026-11-03 v 到 20％ 后改的）
         rows={'%s%d' % (k, n): (lambda src, icon, n: lambda v: dict(src=src, iconId=icon, effectEndurance=-1.0,
                                                                   **{f: (1 + n * v / 100) / (1 + (n - 1) * v / 100) for f in SPELL_RATES}))(
                   src, ICON_ORB if k == 'sorc' and n == 1 else -1, n)
               for n in range(1, ORB_MAX + 1) for k, src in (('sorc', 330010), ('inc', 330500))},
         logic=[dict(type='while', stack='dr_orb', min=n, effect='%s%d' % (k, n)) for n in range(1, ORB_MAX + 1) for k in ('sorc', 'inc')]),
    dict(key='dr_orb_burst', suffix=['Orbburst', '迸发'], slots=('arm', 'talisman'), icon=18000, vals=ORB_BURST,
         zh='环绕身边的法球满了之后，下一次施放魔法或祷告时，能将法球全部射出，每颗造成%s点魔力伤害；一次射出的越多，每颗的伤害越高（每颗＋' + str(ORB_GROW) + '％）',
         en='Once your orbs are full, your next sorcery or incantation releases all of them, each dealing %s magic damage; '
            'the more released at once, the harder each hits (+' + str(ORB_GROW) + '%% per orb)',
         # 用户 2026-11-05「迸发现在收益已经不如保持法球在身上了」→「按C吧」：涌流是百分比加成、射空后重新攒的损失随颗数平方涨，
         # 迸发是固定伤害、只随颗数线性涨（7 颗时每颗要有一发法术伤害的 60% 才回本）→ 底数 ×1.5（60/90/120/160 → 90/135/180/240），
         # 每颗再 ×(1 + 15% × 射出的颗数)（burst_grow，7 颗 ×2.05，回本线降到约 29%）
         # 用户 2026-10-29：改成法球满了之后下次施法触发（以前是后撤步 / 蹲下）。take_full：凝聚那条叠层满了才做，这次施法不另出一颗；
         # 每颗法球射出一颗辉石魔砾（一颗接一颗连发，burst_each），伤害挑 pebble 那套里最接近每颗数值的一档
         # burst_spell：再乘拿着的生成法器的法术补正基数 ÷ 100（用户 2026-11-03「法球发射能吃法杖的魔法加成」，dark_core spellbase.rs）
         logic=[dict(type='on', event='cast_new', take='dr_orb', take_full=1, burst_flat=lambda v: v, burst_kind='pebble', burst_each=1,
                     burst_spell=1, burst_grow=ORB_GROW)]),
    # 用户 2026-11-06 去掉代价（以前魔法、祷告消耗的专注值 +50%）
    # 用户 2026-11-06「星群射出的法球伤害直接按照迸发的最高档定」（以前 160）、「不止 还要吃到迸发每颗的加成」：
    # 一次只射一颗，按射出时身边有几颗算（grow_held，满 7 颗 ×2.05，和迸发一样）
    dict(key='dr_orb_swarm', suffix=['Starswarm', '星群'], slots=('talisman',), icon=18020, vals=(ORB_BURST[-1],) * 4, rarities=('Legendary',),
         keystone=True, zh='环绕身边的法球每2秒会自动射出一颗，攻击周围的敌人（%s点魔力伤害）；身边的法球越多，这一颗的伤害越高（每颗＋' + str(ORB_GROW) + '％），施法也越快（每颗＋5％）',
         en='Every 2 seconds one of your orbs fires itself at nearby enemies (%s magic damage); the more orbs around you, the harder it hits (+' + str(ORB_GROW) + '%% per orb) and the faster you cast (+5%% per orb)',
         logic=[dict(type='on', every=2.0, take='dr_orb', take_max=1, burst_flat=lambda v: v, burst_kind='pebble', burst_each=1, burst_spell=1, burst_grow=ORB_GROW, grow_held=1),
                dict(type='while', cond='casting', stack='dr_orb', min=1, layer_speed=ORB_CAST_SPEED)]),
    # 盈满（原名鹰眼视觉，用户 2026-11-04「名字怪怪的 改一下名字 去除一下负面效果」「随稀有度提高到1234」）：凝聚的法球上限按稀有度多 1~4 颗
    # （凝聚那条叠层的 max_boost 按这条戴着的稀有度取 HAWK_ORBS），以前是多 1 颗、魔法祷告攻击力 -15~-6％。键不变（已有的装备照样认）
    dict(key='dr_hawk', suffix=['Brimming', '盈满'], slots=('leg',), vals=HAWK_ORBS,
         zh='环绕身边的法球上限增加%s颗',
         en='Maximum orbs around you +%s',
         logic=[]),
    # 双异常反应（bd设计.md 3.6）：敌人身上叠两种异常会起反应
    # 用户 2026-11-01：数值 ×3（以前 3 / 4 / 5 / 7）
    dict(key='dr_resonance', suffix=['Frenzied Resonance', '狂乱共鸣'], slots=('head', 'talisman'), icon=18330, vals=(9, 12, 15, 21),
         zh='周围的敌人身上每有一种发作中的异常状态，受到的伤害就提升%s％',
         en='Nearby enemies take %s%% more damage for each status ailment active on them',
         rows={'res': lambda v: [dict(enemy=True, effectEndurance=1.0, **{f: 1 + n * v / 100 for f in CUT_FIELDS}) for n in range(1, 5)]},
         logic=[dict(type='foe_aura', range=30.0, fx='res')]),
    dict(key='dr_shatter', suffix=['Shatter', '碎冰'], slots=('weapon',), vals=(60, 60, 60, 60), rarities=RAR,
         zh='冻伤中的敌人发作出血时，追加出血伤害的%s％，并解除冻伤',
         en='When blood loss procs on a frostbitten enemy, it takes an extra %s%% of the blood loss damage and the frostbite ends',
         logic=[dict(type='foe_react', when='bleed', need='frost', pct=lambda v: v, remove=1, log='碎冰')]),
    # 巨人杀手（反馈表；用户 2026-10-28 定第三下 3%）：命中目标层，按被打敌人的最大血量算，挑最接近的固定扣血档（HP_TIERS）挂给它；
    # 以前每下最多右手武器攻击力的 100%（难度会放大 Boss 血量），用户 2026-11-06「不需要把上限设定为武器面板了」去掉；还受 HP_TIERS 最高档限制
    # 用户 2026-11-13「巨人杀手每次改为0.5% 第3次为1.5%」，同日「回调巨人杀手数值到1%和3%」：每下 1％、第三下共 3％（最早是 1％ / 4％）；
    # vals 第二个数是第三下的总量，规则里加的是差。直接扣血，dark_core 再乘难度的玩家造成伤害倍率
    dict(key='dr_giant', suffix=['Giant Slayer', '巨人杀手'], slots=('weapon',), vals=((1, 3),) * 4, rarities=RAR,
         zh='攻击命中敌人时，追加敌人最大血量%s％的伤害，对同一敌人每第三下改为%s％',
         en="Hits deal extra damage equal to %s%% of the enemy's max HP; every third hit against the same enemy deals %s%% instead",
         logic=[dict(type='on', event='land', target_hp_pct=lambda v: v[0], target_nth=3, target_nth_pct=lambda v: v[1] - v[0], target_reset=5.0)]),
    dict(key='dr_plague', suffix=['Plague Burst', '瘟疫爆发'], slots=('weapon',), vals=(100, 100, 100, 100), rarities=('Legendary',),
         zh='攻击命中中毒或腐败的敌人时，剩余的异常伤害会一次造成（%s％），并解除该异常',
         en='Hitting a poisoned or rotting enemy deals the rest of that ailment at once (%s%%) and ends it',
         logic=[dict(type='on', event='land', target_has=['poison', 'rot'], target_remove=1, target_settle=lambda v: v)]),
    # 用户 2026-11-01：累积 ×5（以前 150）
    dict(key='dr_awaken', suffix=['Rude Awakening', '惊醒'], slots=('weapon',), vals=(750, 750, 750, 750), rarities=('Legendary',),
         zh='攻击命中睡眠中的敌人会将其惊醒，并附加%s点随机异常累积（中毒、腐败、出血或冻伤）',
         en='Hitting a sleeping enemy wakes it and adds %s buildup of a random ailment (poison, rot, blood loss or frost)',
         logic=[dict(type='on', event='land', target_has=['sleep'], target_remove=1, target_buildup=lambda v: v, buildup_kinds=AIL_RANDOM)]),
    # 用户 2026-11-01：累积 ×2（以前 100）
    dict(key='dr_charge', suffix=['Gathered Venom', '积蓄'], slots=('weapon',), vals=(200, 200, 200, 200), rarities=('Legendary',),
         zh='跳跃、蹲下或翻滚后的3秒内，下一次攻击命中敌人时附加%s点随机异常累积（中毒、腐败、出血或冻伤）',
         en='Within 3 seconds after jumping, crouching or rolling, your next hit adds %s buildup of a random ailment (poison, rot, blood loss or frost)',
         rows={'ready': lambda v: dict(iconId=status_icon('charge'), vfxId=VFX_CHARGE, effectEndurance=-1.0)}, events={'crouch': 8001},
         logic=[dict(type='stack', add=['jump', 'crouch', 'roll'], max=1, timeout=3.0, fx='ready'),
                # take_fresh（用户 2026-11-05「积蓄的效果还是会被翻滚触发」）：只认加层之后新出的招打中；翻滚中、之前出的招晚到的命中不算
                dict(type='on', event='land', take='dr_charge', take_fresh=1, target_buildup=lambda v: v, buildup_kinds=AIL_RANDOM)]),
    # 疫病（用户 2026-11-04 的疫病护符，2026-11-06「疫病也应该是词条啊 以后也没有固定护符」改成词条）：这里只要标记，敌人一侧在 rr/plague.py + dark_core plague.rs。
    # 编号锁给了以前的固定护符 4141~4144、标记 7305000~7305003（id_lock.json），老存档的护符不用管
    # 用户 2026-11-13「疫病护符负面效果加强 传说为提高50%基础抗性 普通为100% 其他缩放」：100 / 83 / 67 / 50（以前 30 / 20 / 10 / 0，传说没有代价）
    dict(key='dr_pestilence', suffix=['Pestilence', '疫病'], slots=('talisman',), icon=18400, vals=(100, 83, 67, 50),
         zh='能让所有敌人陷入所有异常状态，且反复发作时抗性不再提升，但基础抗性提升%s％',
         en='Makes every enemy susceptible to every status ailment, and repeated ailments no longer raise their resistance, but raises their base status resistances by %s%%',
         logic=[]),
    dict(key='dr_chain', suffix=['Contagion', '连锁'], slots=('talisman',), icon=18340, vals=(80, 80, 80, 80), rarities=('Legendary',),
         keystone=True, zh='敌人发作异常状态时，周围6米内的其他敌人也会累积%s点同种异常，但自身的异常抗性降低50％',
         en='When an ailment procs on an enemy, other enemies within 6 m gain %s buildup of the same ailment, but your own resistances are halved',
         mark={f: 0.5 for f in REGIST},
         logic=[dict(type='foe_spread', range=6.0, amount=lambda v: v, kinds=['poison', 'rot', 'bleed', 'frost', 'sleep', 'madness'], log='连锁')]),
    # 空战（bd设计.md 3.7）：尽量留在空中打
    # 凌空（用户 2026-10-29 重新设计，选了「命中后接跳」）：原版的跳劈大多在落地那一下才打中，以前的「空中命中后再跳」几乎不会发生，
    # 而且在空中 / 地上的判断读的物理模块标记在空中也是「在地上」，层刚加上就清掉了。现在：
    #   - 跳跃攻击（玩家脚本 JumpAttackForm 不是 0 时每帧发 jatk，落地攻击也算）命中敌人 → 0.5 秒的「可以接跳」（风之结晶露滴的光效，
    #     导出给玩家脚本 air_ready）；这期间按跳跃立刻起跳（地上直接跳、空中先切到下落再跳；照常扣跳跃的精力，天堑照样补回），发 air_jump；
    #   - 以前每次接跳还叠一层「连跳」加跳跃攻击的攻击力；用户 2026-10-30「凌空去掉增伤效果吧 只保留机制」去掉了
    #     （状态图标表里的 aerial 留着没用：删掉后面几个的图标编号会挪）
    #   - 用户 2026-11-03「凌空随稀有度提高减少跳劈精力消耗 最高40%」：人在空中时（玩家脚本每帧发 air）挂 consumeStaminaRate
    #     （行为参数里的精力消耗 × 倍率，跳劈的消耗就在行为参数里；不分动作，所以只在空中挂，这时候能出的基本只有跳劈）
    dict(key='dr_aerial', suffix=['Skybound', '凌空'], slots=('arm', 'talisman'), icon=18800, vals=(10, 20, 30, 40),
         zh='跳跃攻击命中敌人后，立刻按下跳跃能再次起跳；跳跃攻击消耗的精力减少%s％',
         en='After a jump attack hits an enemy, press jump right away to leap again; jump attacks cost %s%% less stamina',
         rows={'ready': lambda v: [dict(iconId=-1, vfxId=VFX_AERIAL, effectEndurance=-1.0)],
               'cheap': lambda v: dict(iconId=-1, effectEndurance=-1.0, consumeStaminaRate=1 - v / 100)},
         hks={'air_ready': 'ready'},
         logic=[dict(type='stack', add=['land'], cond='jatk', max=1, timeout=0.5, fx='ready', log='凌空（可以接跳）'),
                dict(type='on', event='air_jump', take='dr_aerial', take_max=1),
                dict(type='while', cond='air', effect='cheap')]),
    dict(key='dr_meteor', suffix=['Falling Star', '坠星'], slots=('body',), vals=(20, 30, 40, 50),
         zh='跳跃攻击落地时能震出冲击波，造成右手武器攻击力%s％的物理伤害，并削减强韧度',
         en='Landing a jump attack releases a shockwave dealing %s%% of your right-hand weapon attack as physical damage and breaking poise',
         # 冷却 0.5 秒（用户 2026-11-12「坠星为什么连续跳劈时没有触发」：原地跳的跳劈落地以前不发 jland，现在玩家脚本也发了（sc/hks.py JLAND_FIX），
         # 向前跳的跳劈落地两处都会发，冷却防止同一次落地放两下）
         logic=[dict(type='on', event='jland', burst=lambda v: v, cooldown=0.5)]),   # 冲击波子弹自带原版特效（SHOCK_SFX）
    # 用户 2026-11-12「浮空加上负面效果削韧减少30%」：戴着就一直挂 weak（削韧 ×0.7，同千钧 / 蛮牛的 saAttackPowerRate）
    dict(key='dr_float', suffix=['Levitation', '浮空'], slots=('head',), vals=(0, 0, 0, 0), rarities=('Rare', 'Epic', 'Legendary'),
         zh='每次跳跃后，在空中第一次受到攻击不会被打断（落地前有效），但削韧降低30％',
         en='After each jump, the first hit you take in the air does not interrupt you (until you land), but your attacks deal 30%% less poise damage',
         rows={'look': lambda v: dict(iconId=20430, vfxId=8570, effectEndurance=-1.0),
               'weak': lambda v: dict(effectEndurance=-1.0, saAttackPowerRate=0.7)}, hks={'float_on': 'look'},
         logic=[dict(type='stack', add=['jump'], clear=['ground', 'float_hit'], max=1, fx='look', log='浮空'),
                dict(type='while', effect='weak')]),
    dict(key='dr_chasm', suffix=['Skyward Chasm', '天堑'], slots=('talisman',), icon=18350, vals=(25, 25, 25, 25), rarities=('Legendary',),
         keystone=True, zh='跳跃不再消耗精力，跳跃攻击的攻击力提升%s％，但站在地面上时（跳跃攻击除外）攻击力降低30％',
         en='Jumping no longer costs stamina and jump attacks deal %s%% more, but your attack drops by 30%% while standing on the ground (except jump attacks)',
         # 降攻：站在地上、但跳劈落地那一下（玩家脚本落地攻击的状态里每帧发 jland）不算（用户 2026-10-28：跳劈最后一下在地上也吃了减伤）；
         # 光效、图标照桂奥尔的咆哮（原版效果 1709000 挂给敌人的降攻 / 降防）；跳劈加成照钩爪护符带它的图标（用户 2026-10-28 要的）
         rows={'jatk': lambda v: dict(src=321800, iconId=ICON_CLAW, effectEndurance=-1.0, **{f: 1 + v / 100 for f in ATK_RATES}),
               'ground': lambda v: dict(effectEndurance=-1.0, iconId=ICON_WEAKEN, vfxId=VFX_WEAKEN, **{f: 0.7 for f in ATK_RATES})},
         logic=[dict(type='on', event='jump', stamina_back=100), dict(type='while', effect='jatk'),
                dict(type='while', cond='ground', unless='jland', effect='ground')]),
    # 完美闪避（bd设计.md 3.8）：贴着敌人的攻击翻滚。见切是近似（原版无敌是碰撞层过滤，躲掉的攻击不产生命中）：
    # dll\dark_core 在翻滚 / 后撤步的无敌帧内看敌人的子弹位置、近战的攻击判定（foewin.txt：判定时间段；朝向、距离），见 rules.rs 的 mikiri_reason
    dict(key='dr_mikiri', suffix=['Mikiri', '见切'], slots=('leg', 'talisman'), icon=18110, vals=((90, 1), (90, 2), (90, 3), (90, 4)),
         zh='翻滚或后撤步的无敌时间里，若敌人的近战攻击或飞来的攻击正好落在身上，即为见切：周围8米内的敌人动作减慢%s％，持续%s秒；'
            '减速结束后5秒内不会再次见切',
         en="If an enemy's melee attack or a projectile lands on you during the invincibility of a roll or backstep, it is a Mikiri: "
            'enemies within 8 m slow down by %s%% for %s seconds; no new Mikiri for 5 seconds after the slow ends',
         rows={'flash': lambda v: dict(vfxId=VFX_MIKIRI, effectEndurance=1.0)},
         logic=[dict(type='on', event='mikiri', foe_slow=lambda v: v[0], foe_radius=8.0, foe_secs=lambda v: v[1],
                     rest=5.0, effect='flash')]),   # rest：减速 / 停住结束后 5 秒内不再见切（用户 2026-10-28）
    # 用户 2026-11-24「见切后下一击从80的均值改为40的均值」：攻击力、削韧都减半（30 / 45 / 60 / 80 → 15 / 22 / 30 / 40）
    dict(key='dr_zanshin', suffix=['Zanshin', '残心'], slots=('arm',), vals=((15, 15), (22, 22), (30, 30), (40, 40)),
         zh='见切后的3秒内，下一次攻击能提升%s％攻击力与%s％削韧',
         en='Within 3 seconds after a Mikiri, your next attack deals +%s%% damage and +%s%% poise damage',
         rows={'buff': lambda v: dict(iconId=ICON_ATK, vfxId=VFX_BREAK, effectEndurance=-1.0, saAttackPowerRate=1 + v[1] / 100,
                                      **{x: 1 + v[0] / 100 for x in ATK_RATES})},
         logic=[dict(type='stack', add=['mikiri'], consume=['attack'], max=1, timeout=3.0, fx='buff')]),
    # 用户 2026-11-24「见切后加速改为30%」：50 → 30
    dict(key='dr_shadowstep', suffix=['Shadowstep', '影步'], slots=('talisman',), icon=18120, vals=(30, 30, 30, 30), rarities=('Legendary',),
         # 用户 2026-11-12「影步改一下效果吧 改为见切后自身动作加快50%3s好了」：以前是见切时周围敌人停住 3 秒（见切那条规则的 freeze_key），
         # 停住的敌人掉出扫描列表时会一直卡着；2026-11-06 去掉过代价（翻滚的无敌时间减半）。加快同处刑（haste），两条同时在就相乘。
         # 同一天「影步必须有见切才生效」：requires=dr_mikiri（dark_core 缺见切就当影步没戴），冷却跟见切的 5 秒走
         keystone=True, zh='同时带有「见切」时，见切后的3秒内，自身所有动作能加快%s％',
         en='While Mikiri is also equipped, all your actions play %s%% faster for 3 seconds after a Mikiri',
         rows={'haste': lambda v: dict(iconId=ICON_HASTE, vfxId=VFX_AERIAL, effectEndurance=3.0)},
         logic=[dict(type='on', event='mikiri', effect='haste', haste=lambda v: v, haste_secs=3.0, requires='dr_mikiri', log='影步')]),
    # ---- 重击处决（bd设计.md 3.10，用户 2026-10-28）：蓄力重击 → 崩架势 → 致命一击。
    # 玩家脚本：charging（按住重击蓄力中）、charged（满蓄力放出，没中途松开）、crit（致命一击）；崩架势 break 是 dark_core 看敌人架势值判断的。
    # 削韧照「蛮牛」用 saAttackPowerRate，满蓄力时由 dark_core 挂效果（用户 2026-10-28：最高 30% 就够）
    dict(key='dr_crack', suffix=['Bonebreaker', '碎骨'], slots=('arm', 'talisman'), icon=18300, vals=(10, 15, 20, 30),
         zh='满蓄力重击能提升%s％削韧', en='Fully charged heavy attacks deal +%s%% poise damage',
         # 用户 2026-11-13（特效提案）：满蓄力出招期间身上挂岩棘裂纹露滴那层光（「削韧」的光，残躯、精准格挡也用它），出招完就收
         rows={'crack': lambda v: dict(vfxId=VFX_POISE, effectEndurance=-1.0, saAttackPowerRate=1 + v / 100)},
         logic=[dict(type='while', cond='charged', effect='crack')]),
    dict(key='dr_opening', suffix=['Opening', '破绽'], slots=('head', 'talisman'), icon=18660, vals=(10, 15, 20, 25),
         zh='被你打崩架势的敌人，5秒内受到的伤害提升%s％', en='Enemies whose stance you break take %s%% more damage for 5 seconds',
         rows={'open': lambda v: dict(enemy=True, vfxId=VFX_WEAKEN, effectEndurance=5.0, **{f: 1 + v / 100 for f in CUT_FIELDS})},
         logic=[dict(type='on', event='break', target_on='break', target_fx='open')]),
    # 处刑（用户 2026-11-06「把处决后对周围敌人造成伤害的词条改为加速自身所有动作20%持续10s」）：致命一击的动作结束时开始算（处决动作和对方同步，中途不能变速）
    dict(key='dr_execute', suffix=['Execution', '处刑'], slots=('body', 'talisman'), icon=18320, vals=(5, 10, 15, 20),
         zh='施展致命一击后的10秒内，自身所有动作能加快%s％',
         en='For 10 seconds after a critical hit, all your actions play %s%% faster',
         rows={'haste': lambda v: dict(iconId=ICON_HASTE, vfxId=VFX_AERIAL, effectEndurance=10.0)},
         logic=[dict(type='on', event='crit', when='end', effect='haste', haste=lambda v: v, haste_secs=10.0, log='处刑')]),
    # 2026-11-06 用户「那个处决后恢复精力和专注的词条加上回复血量」：血量按最大值的百分比，略高于饮血（处决比击杀难打出来）
    dict(key='dr_composure', suffix=['Composure', '余裕'], slots=('arm',), vals=((20, 5, 3), (30, 8, 5), (40, 11, 7), (50, 15, 10)),
         zh='施展致命一击后，能恢复%s点精力、%s点专注值与%s％血量', en='Critical hits restore %s stamina, %s FP and %s%% HP',
         # 用户 2026-11-13（特效提案）：回复时一圈祷告「回复」的金色光（原版就是开头特效，放完就完）
         rows={'flash': lambda v: dict(vfxId=VFX_HEAL_GOLD, effectEndurance=0.5)},
         logic=[dict(type='on', event='crit', stamina=lambda v: v[0], fp=lambda v: v[1], hp_pct=lambda v: v[2], effect='flash')]),
    # 乘势（用户 2026-11-04「连击后强化下一次战技吧」「三层 每层10% 挨打减少一层 满层战技有霸体……不是一次性的」「那就不要减伤了 稀有度只改加伤数值」）：
    # 胴甲，替换霸王（霸王停用：RETIRED_KEYS，已有的装备照旧）。普通攻击（轻 / 重 / 跳跃 / 冲刺 / 防反，玩家脚本 DR_ClashFrame 那些状态发 swing）
    # 打中敌人（land）加一层，最多 3 层，每层战技伤害 +v％（照亚历山大的碎片 312310：五种攻击倍率 + 只管战技的攻击子类别 112 / 111）；
    # 受击（hit）掉一层，战技中不掉（不然满层战技第一下受击就没了霸体）；战技结束（skill 事件结束）时全部用掉；
    # 满 3 层时战技期间（while skill + 叠层 ≥ 3）挂 full，玩家脚本 DR_DamageLevel 看到它会硬直的受击都按无硬直（整个战技，不是一次性的）
    dict(key='dr_momentum', suffix=['Momentum', '乘势'], slots=('body',), vals=(6, 7, 8, 10),
         zh='普通攻击命中敌人时能积蓄一层势，每层提升下一次战技伤害%s％，最多3层；受到攻击时会失去一层，积满3层时使出的战技不会被打断',
         en="Landing normal attacks builds Momentum (up to 3 stacks), each raising your next skill's damage by %s%%; "
            'getting hit loses a stack, and at 3 stacks your skill cannot be interrupted',
         rows={'layer': lambda v: [dict(iconId=status_icon('momentum', n), effectEndurance=-1.0, magicSubCategoryChange1=112,
                                        magicSubCategoryChange2=111, **{x: 1 + n * v / 100 for x in ITEM_RATES}) for n in range(1, 4)],
               'full': lambda v: dict(iconId=-1, vfxId=8570, effectEndurance=-1.0)},
         hks={'momentum_full': 'full'},
         logic=[dict(type='stack', add=['land'], cond='swing', max=3, fx='layer', log='乘势'),
                dict(type='on', event='hit', unless=['skill'], take='dr_momentum', take_max=1),
                dict(type='on', event='skill', when='end', take='dr_momentum'),
                dict(type='while', cond='skill', stack='dr_momentum', min=3, effect='full')]),
    # 千钧（用户 2026-11-01 改成命中时生效）：满蓄力这一击期间 dark_core 给身边 6 米内的敌人挂 brk（原版 saReceiveDamageRate：
    # 受到的削韧 ×1000，2 秒，这一击结束就摘），打中的当场由引擎判崩架势，没打中的不变（命中后再改架势值引擎当场不判崩）；
    # Boss（NpcParam 的 isSoulGetByBoss）不挂，出手时先扣上限的 #%，打中时引擎连同这一击的削韧一起判崩，没打中就加回去（rules.rs poise_pre）
    # 用户 2026-11-12「千钧数值上调到40%」（以前 30%）
    dict(key='dr_thousand', suffix=['Thousandweight', '千钧'], slots=('talisman',), icon=18370, vals=(40, 40, 40, 40), rarities=('Legendary',),
         keystone=True, zh='满蓄力重击必定打崩敌人的架势（Boss改为削减强韧度上限的%s％），但其他攻击不再削减敌人的强韧度',
         en="Fully charged heavy attacks always break the enemy's stance (bosses instead lose %s%% of their maximum poise), "
            'but your other attacks no longer break poise',
         rows={'none': lambda v: dict(effectEndurance=-1.0, saAttackPowerRate=0.0),
               'brk': lambda v: dict(enemy=True, effectEndurance=2.0, saReceiveDamageRate=1000.0),
               'glow': lambda v: dict(vfxId=VFX_POISE, effectEndurance=-1.0)},   # 用户 2026-11-13（特效提案）：满蓄力出招期间的光，同碎骨
         logic=[dict(type='while', cond='charged', poise_radius=6.0, poise_boss_pct=lambda v: v, poise_fx='brk', effect='glow'),
                dict(type='while', unless='charged', effect='none')]),
    # ---- 远程与道具（bd设计.md 3.11）：箭叠猎印 → 投掷道具引爆 / 近战收割，命中返还弹药，道具和弓交替加伤。
    # 玩家脚本：shot（射箭）、throw（投掷道具）、aroma（调香）；命中：重随给箭 / 投掷道具的子弹加「命中时给射手挂效果」→ shot_hit / item_hit。
    # 加伤照原版「翔空箭」（攻击子类别 105 / 113 / 118）；道具加伤只认统一的道具标签 ITEM_CAT（见 ITEM_DMG）
    # 用户 2026-11-10「猎印受到伤害倍率也翻倍」：每层 2 / 3 / 4 / 5 → 4 / 6 / 8 / 10
    dict(key='dr_hunt', suffix=["Hunter's Mark", '猎印'], slots=('head', 'talisman'), icon=18690, vals=(4, 6, 8, 10),
         zh='箭或弩箭命中敌人时，能给它挂上一层猎印（最多3层，持续10秒），每层使它受到的伤害提升%s％',
         en="Arrow and bolt hits put a Hunter's Mark on the enemy (up to 3, 10 seconds); each mark makes it take %s%% more damage",
         rows={'mark': lambda v: [dict(enemy=True, vfxId=VFX_HUNT[n - 1], effectEndurance=10.0, **{f: 1 + n * v / 100 for f in CUT_FIELDS})
                                  for n in range(1, 4)]},
         logic=[dict(type='on', event='land', cond='shot_hit', target_mark='dr_hunt', mark_max=3, mark_secs=10.0, mfx='mark')]),
    # 用户 2026-11-05「猎印引爆伤害以当前武器面板作为基础伤害 然后每多一层伤害*2」「引爆从60开始涨吧」「传说封顶在100%」「引爆改为每层1.3倍吧 现在是有点爆炸了」：面板的百分比，普通 60 ~ 传说 100，每多一层 × 1.3（以前是每层固定 60 / 90 / 120 / 160 × 物理的属性倍率）
    # 用户 2026-11-13「引爆改为每层2.5倍」（2026-11-11 是 1.5、更早 1.3）：每多一层 × DETONATE_GROW，说明按它写
    dict(key='dr_detonate', suffix=['Detonation', '引爆'], slots=('leg', 'talisman'), icon=18830, vals=(60, 75, 90, 100),
         zh='投掷壶、飞刀等投掷道具命中带猎印的敌人时，能引爆猎印，造成右手武器攻击力%s％的伤害，每多一层，伤害再提升至' + ('%g' % DETONATE_GROW) + '倍',
         en="Throwing pots, knives and other throwables detonate Hunter's Marks on the enemy they hit, dealing %s%% of your right-hand weapon attack; each extra mark multiplies the damage by " + ('%g' % DETONATE_GROW),
         rows={'burst': hunt_burst_rows},
         logic=[dict(type='on', event='land', cond='item_hit', target_take='dr_hunt', take_pct=lambda v: v, take_grow=DETONATE_GROW, tfx='burst')]),
    # 用户 2026-11-06「提升一下数值到100%」：传说每层 100％，其余按原来的比例（以前 10 / 15 / 20 / 25）；
    # 2026-11-13「近身基础倍率为200%」：传说 200％，其余同比例（以前 40 / 60 / 80 / 100）
    dict(key='dr_close', suffix=['Closing In', '近身'], slots=('body',), vals=(80, 120, 160, 200),
         zh='以近战攻击打中带猎印的敌人时，能清除猎印，每层追加右手武器攻击力%s％的伤害',
         en="Melee hits consume the Hunter's Marks on the enemy, dealing an extra %s%% of your right-hand weapon attack per mark",
         rows={'burst': hunt_burst_rows},
         logic=[dict(type='on', event='land', unless=['shot_hit', 'item_hit', 'cast_hit', 'cast'], target_take='dr_hunt', take_pct=lambda v: v, tfx='burst')]),
    # 用户 2026-11-01：几率提高到 普通 50% / 传说 80%（中间两档等距补齐）；以前是 10 / 15 / 20 / 30
    dict(key='dr_salvage', suffix=['Salvage', '回收'], slots=('leg', 'talisman'), icon=18870, vals=(50, 60, 70, 80),
         zh='箭、弩箭或投掷道具命中敌人时，有%s％的几率返还一个', en='Arrows, bolts and throwables that hit have a %s%% chance to be returned',
         logic=[dict(type='on', event=ev, chance=lambda v: v, refund=10.0) for ev in ('shot_hit', 'item_hit')]),
    # 用户 2026-11-10「交替数值翻倍」：10 / 15 / 20 / 25 → 20 / 30 / 40 / 50
    dict(key='dr_alternate', suffix=['Alternation', '交替'], slots=('head', 'talisman'), icon=18840, vals=(20, 30, 40, 50),
         zh='使用投掷道具或调香后的20秒内，能提升%s％箭与弩箭的伤害；射箭命中后的20秒内，能提升%s％道具伤害',
         en='For 20 seconds after using a throwable or perfume, arrows and bolts deal +%s%%; for 20 seconds after an arrow hits, item damage +%s%%',
         # 用户 2026-11-13（特效提案）：只在获得状态时闪一下（光效行只有开头特效），持续期间看图标
         rows={'bow': lambda v: dict(iconId=ICON_ATK, vfxId=VFX_ALT_BOW, effectEndurance=-1.0, magicSubCategoryChange1=105, magicSubCategoryChange2=113,
                                     magicSubCategoryChange3=118, **{x: 1 + v / 100 for x in ATK_RATES}),
               'item': lambda v: dict(iconId=ICON_ATK, vfxId=VFX_ALT_ITEM, effectEndurance=-1.0, magicSubCategoryChange1=ITEM_CAT,
                                      **{x: 1 + v / 100 for x in ATK_RATES})},
         logic=[dict(type='stack', add=['throw', 'aroma'], max=1, timeout=20.0, fx='bow'),      # 用户 2026-11-05：5 秒 → 20 秒
                dict(type='stack', add=['shot_hit'], max=1, timeout=20.0, fx='item')]),
    dict(key='dr_volley', suffix=['Volley', '连射'], slots=('arm',), vals=(4, 6, 8, 10),
         zh='弓箭连续命中敌人时，每次能加快%s％弓的拉弦与弩的装填（最多5层，3秒没有命中就会散去）',
         en='Each consecutive arrow or bolt hit speeds up drawing and reloading by %s%% (up to 5 stacks, fades after 3 seconds without a hit)',
         logic=[dict(type='stack', add=['shot_hit'], max=5, timeout=3.0, reload_speed=lambda v: v, log='连射'),
                dict(type='while', cond='reload')]),          # 只为了让玩家脚本发「拉弓中」（runtime 只在拉弓中乘速度）
    # 相杀（用户 2026-11-04，参考剑心修罗的「抗衡」）：臂甲 / 护符，只有传说、规则改变器。轻击 / 重击（含冲刺、翻滚 / 后撤、跳跃、防御反击；
    # 不含战技、处决、射击）出手时：按这个动作的攻击判定，判定开始前 0.2 秒到判定结束后 0.15 秒（用户 2026-11-04「那就按照攻击判定算」，
    # 同天「前后再加0.05s吧」从 0.15 / 0.1 加宽：
    # 原来按动作 25%~75%，后摇占了大半、重击的窗口在判定之后才开、连击时被下一刀切掉；判定表 dev/item_system/build_hit_windows.py，
    # 表里没有的动作（没有任何判定：蓄力循环、翻滚、站立…）不开；用户 2026-11-04「还有查不到动作的吗」后去掉了按进度的退路），受击不会被打断、出原版防御火花，
    # 受到的伤害按拿着的那把武器的防御时减伤率减：轻击一半、重击全额（重击按 1 倍，用户 2026-11-04）。
    # 玩家脚本（sc/hks.py DR_ClashFrame）按状态发 clash_<l|h|hc>_<r|l>（轻击 / 重击 / 满蓄力重击 × 右手 / 左手武器）；
    # 积木引擎按攻击判定挂 l_r … h_l 四行（rules.rs 的 while hit）；这四行的减伤由 dll\dark_core clash.rs 按武器改（clash.txt）；
    # 不被打断、火花在 DR_DamageLevel（DR_AnyOn clash_*）
    # 无形（用户 2026-11-06「能不能加个词条实现黑夜魔法的效果 不会被读指令」）：这里只要一个标记，逻辑在玩家脚本（sc/hks.py AFFIX_UNSEEN）和 dark_core unseen.rs。
    # 同日用户「只管远程箭和法术道具 近战不屏蔽」「喝药灵药也不拦截」。只出传说：不加伤害，但敌人不再抢攻、闪避，改的是整个战斗的规则。
    # 2026-11-19 用户「无形护符设定为和其他护符都不冲突」：不进规则改变器的共用组，自己一个组（own_group）
    dict(key='dr_unseen', suffix=['Unseen', '无形'], slots=('talisman',), icon=18150, vals=(0, 0, 0, 0), rarities=('Legendary',),
         keystone=True, own_group=True, zh='能让敌人难以察觉你的施法，也不会对你的箭矢、法术和投掷道具作出反应',
         en='Enemies can barely read your casts, and do not react to your arrows, spells or thrown items',
         logic=[]),
    dict(key='dr_clash', suffix=['Clash', '相杀'], slots=('arm', 'talisman'), icon=18290, vals=(60, 60, 60, 60), rarities=('Legendary',),
         keystone=True, zh='轻击或重击出手时能与敌人的攻击相杀：受到攻击时不会被打断，并减少%s％伤害',
         en='Light and heavy attacks clash with enemy attacks mid-swing: hits do not interrupt you and damage taken is reduced by %s%%',
         rows={**{k: (lambda v: dict(iconId=-1, effectEndurance=-1.0)) for k in ('l_r', 'l_l', 'h_r', 'h_l')},
               'spark_r': lambda v: dict(iconId=-1, effectEndurance=1.2, vfxId=VFX_CLASH_R),
               'spark_l': lambda v: dict(iconId=-1, effectEndurance=1.2, vfxId=VFX_CLASH_L)},
         hks={'clash_lr': 'l_r', 'clash_ll': 'l_l', 'clash_hr': 'h_r', 'clash_hl': 'h_l', 'clash_spark_r': 'spark_r', 'clash_spark_l': 'spark_l'},
         logic=[dict(type='while', cond='clash_%s_%s' % (k, h), hit=[0.2, 0.15], effect='%s_%s' % (k[0], h))
                for k in ('l', 'h', 'hc') for h in ('r', 'l')]),
    dict(key='dr_quiver_inf', suffix=['Endless Quiver', '无尽箭袋'], slots=('talisman',), icon=18340, vals=(30, 30, 30, 30), rarities=('Legendary',),   # 用户 2026-11-05：10% → 30%
         keystone=True, zh='箭、弩箭和投掷道具不会再消耗，并能提升%s％箭、弩箭和道具的伤害',
         en='Arrows, bolts and throwables are no longer used up, and arrow, bolt and item damage is increased by %s%%',
         # dll\dark_core 的 free 规则：开游戏的「箭矢不消耗」/ 投掷动作里「道具不消耗」
         # （2026-10-29 用户：戴着还是会消耗——以前数背包返还，连着射 / 投、用掉最后一个时返还不了）。
         # 用户 2026-11-04「无尽箭袋护符取消专注值消耗 增加10%道具伤害」「箭矢伤害也加」：不再扣专注（fp 0）；
         # 道具伤害照「道具伤害」词条（攻击子类别 ITEM_CAT 的五种攻击倍率），直接写在护符的标记效果上（spCategory 0，和别的道具伤害词条叠乘）；
         # 箭矢伤害照原版锐利箭护符 321500（子类别 105 / 113 / 118，三个位都用了，所以另一行），积木引擎戴着时一直挂
         mark={**{x: (lambda v: 1.0 + v / 100.0) for x in ITEM_RATES}, 'magicSubCategoryChange1': ITEM_CAT},
         rows={'arrow': lambda v: dict(iconId=-1, effectEndurance=-1.0, magicSubCategoryChange1=105, magicSubCategoryChange2=113,
                                       magicSubCategoryChange3=118, **{x: 1.0 + v / 100.0 for x in ITEM_RATES})},
         logic=[dict(type='free', fp=lambda v: 0), dict(type='while', effect='arrow')]),
]
CUSTOM_STACKS = 10                                      # 优雅最多几层
COCKTAIL = (                                           # 鸡尾酒的效果：(名字, 图标, 普通 ~ 传说的数值)
    ('atk', 20406, (3, 5, 7, 10)),                     # 造成的伤害 +%
    ('cut', 20408, (3, 5, 7, 10)),                     # 受到的伤害 -%
    ('stam', 20410, (5, 8, 11, 15)),                   # 精力恢复速度 +
    ('hp', 20411, (0.3, 0.5, 0.7, 1.0)),               # 每秒回复最大血量的 %
    ('fp', 20412, (2, 3, 4, 5)),                       # 每秒回复专注值
    ('poise', 20465, (5, 8, 11, 15)),                  # 削韧 +%
)
CUSTOM = [
    # 疾风：2026-11-15 用户「改为四个部位都可出现并且可以叠加」（以前只出在护符、腿甲，同名取最高）
    dict(key='dr_gale', run='gale', suffix=['Gale', '疾风'], slots=('talisman', 'head', 'body', 'arm', 'leg'), icon=18560, vals=(4, 6, 9, 12),
         zh='能提升%s％的移动速度', en='Raises movement speed by %s%%'),
    dict(key='dr_wolf', run='wolf', suffix=['Wolf Soul', '狼魂'], slots=('talisman', 'arm'), icon=18580, vals=(20, 14, 8, 0),
         zh='攻击途中也能随时举起防御，中断攻击（每次消耗%s点精力）',
         en='You can raise your guard at any point during an attack, cancelling it (costs %s stamina)',
         zh0='攻击途中也能随时举起防御，中断攻击', en0='You can raise your guard at any point during an attack, cancelling it'),
    dict(key='dr_leap', run='leap', suffix=['Wild Goose', '飞鸿'], slots=('leg',), vals=(0.2, 0.3, 0.4, 0.5),
         zh='起跳后的%s秒内能处于无敌状态', en='Grants invincibility for %s seconds after jumping'),
    dict(key='dr_overlord', run='overlord', suffix=['Overlord', '霸王'], slots=('body',), vals=(10, 15, 20, 30),
         # 忍耐的图标、光效（不限时，霸体在的时候积木引擎挂着；armor = 战技中、霸体还没用掉，玩家脚本 DR_SkillArmor 发）
         rows={'look': lambda v: dict(iconId=20430, vfxId=8570, effectEndurance=-1.0)},
         logic=[dict(type='while', cond='armor', effect='look')],
         zh='使出战技时能获得一次霸体，挡下一次硬直后消失；霸体期间受到的伤害减少%s％',
         en='Using a skill grants hyperarmor that absorbs one stagger; while it lasts, damage taken is reduced by %s%%'),
    dict(key='dr_elegance', run='elegance', suffix=['Elegance', '优雅'], slots=('head',), vals=(2, 3, 4, 5),
         zh='战斗中每做满1秒一种肢体动作，就能在20秒内提升%s％攻击力（每种肢体动作叠加一次）',
         en='In combat, each gesture held for 1 second raises attack power by %s%% for 20 seconds (once per gesture)'),
    dict(key='dr_cocktail', run='cocktail', suffix=['Cocktail', '鸡尾酒'], slots=('head', 'body', 'arm', 'leg'), vals=(1, 2, 3, 4),
         zh='每隔10秒随机获得一种增益，持续10秒\n攻击力＋%s％／受到的伤害－%s％／精力恢复＋%s／每秒回复%s％血量／每秒回复%s点专注值／削韧＋%s％',
         en='Every 10 seconds, gain a random boon for 10 seconds\nDamage +%s%% / damage taken -%s%% / stamina recovery +%s / '
            '%s%% HP per second / %s FP per second / poise damage +%s%%'),
    dict(key='dr_timeslip', run='timeslip', suffix=['Time Slip', '时隙'], slots=('talisman',), icon=18460, vals=(10, 15, 20, 25),
         zh='能加快周围8米内所有角色的动作，不分敌我（＋%s％）', en='Speeds up the actions of every character within 8 m, friend or foe (+%s%%)'),
    # 混沌（2026-11-06 改成积木规则）：以前标记自己带 152 + 命中时触发挂减抗，游戏对这种效果只认一个（rr/atkfwd.py），
    # 和听命中的词条标记互相顶：排在后面减抗挂不上，排在前面所有听命中的词条都不触发。现在标记也只是听命中，减抗由 dark_core 挂
    # 用户 2026-11-24「混沌护符加一个负面效果 受到伤害+30% 不用波动」：戴着就一直挂 frail（受到的伤害 ×1.3，同怒意的 DEF_RATES），
    # 每个稀有度一样（同浮空的「削韧降低30％」）
    dict(key='dr_chaos', run='chaos', suffix=['Chaos', '混沌'], slots=('talisman',), icon=18311, vals=(10, 15, 20, 30),
         rows=dict(debuff=lambda v: dict(enemy=True, effectEndurance=10.0, **{x: 1.0 + v / 100.0 for x in STATUS_DEF}),
                   frail=lambda v: dict(effectEndurance=-1.0, **{x: 1.3 for x in DEF_RATES})),
         logic=[dict(type='on', event='land', target_fx='debuff'), dict(type='while', effect='frail')],
         zh='被命中的敌人在10秒内更容易陷入异常状态（异常累积＋%s％），但自身受到的伤害增加30％',
         en='Enemies you hit become more susceptible to every status ailment for 10 seconds (buildup +%s%%), but you take 30%% more damage'),
    dict(key='dr_deft', run='deft', suffix=['Deft Hands', '巧手'], slots=('arm',), vals=(15, 22, 30, 40),
         zh='能加快弩的装填与弓的拉弦（＋%s％）', en='Speeds up crossbow reloading and bow drawing (+%s%%)'),
    # 寸劲（原名沉稳；用户 2026-11-05「重蓄霸体的词条改为减少重蓄所需时间传说最高50%每级10%」、改名「寸劲」）：以前是按住蓄力时霸体 + 减伤（积木 while charging），
    # 改成蓄力所需时间减少 20 / 30 / 40 / 50％：玩家脚本蓄力中发 charging 脉冲，dll\dark_core（runtime.rs quickcharge）这时把玩家的动作速度
    # ×1 / (1 − 数值％)（同巧手的做法，和时隙、巧手的倍数相乘）。标记效果照旧（id_lock 按键 sp/dr_steady/… 锁），已有的沉稳装备直接变成寸劲
    # 用户 2026-11-24「蓄力重击加快从50%变为20%」：传说 50 → 20，其他档同比例（× 0.4）
    dict(key='dr_steady', run='quickcharge', suffix=['Inch Power', '寸劲'], slots=('leg',), vals=(8, 12, 16, 20),
         zh='能缩短重击蓄力所需的时间（减少%s％）', en='Shortens the time needed to charge heavy attacks (by %s%%)'),
    # 用户 2026-11-12「碾压传说伤害改为200%」「其他稀有度一起缩放」：以前 30 / 40 / 50 / 60%，都 × 200/60；伤害走实时计算（CRUSH_LIVE_N）
    dict(key='dr_crush', run='crush', suffix=['Crush', '碾压'], slots=('talisman', 'body'), icon=18590, vals=(100, 133, 167, 200),
         zh='翻滚不再有无敌时间，但能冲撞周围的敌人，造成右手武器攻击力%s％的物理伤害，并削减强韧度',
         en='Rolls lose their invincibility, but slam into nearby enemies for %s%% of your right-hand weapon attack as physical damage, '
            'breaking their poise'),
    # 拾遗（用户 2026-10-29，参考 ErdTools 的自动拾取）：dark_core pickup.rs 做（戴着时自动执行物品类交互、范围放大、不播拾取动作）。
    # 只管地上物品和敌人掉落（采集材料 ErdTools 已经在做）；范围 4 / 6 / 8 / 10 米、战斗中也吸（都是用户定的）。图标借银色粪金龟（物品发现力）
    dict(key='dr_magnet', run='magnet', suffix=['Scavenging', '拾遗'], slots=('talisman',), icon=18110, vals=(4, 6, 8, 10),
         zh='能自动拾取周围%s米内地上的物品和敌人掉落的物品，不必弯腰去捡',
         en='Automatically picks up items on the ground and enemy drops within %s m, without bending down'),
]
# 新词条共用的行（extract_affixes.py custom_shared）：
#   玩家脚本挂的「脉冲」标记（翻滚中 / 弩换弹、拉弓中 / 攻击开始 / 受击），DLL 看到才做；
#   碾压的冲击波：按伤害分 CRUSH_TIERS 档（DLL 读右手武器攻击力 × 稀有度百分比，挑最接近的一档），照「魔力爆炸」的子弹复制、改成物理；
#   血狂（武器的特殊效果词条位，weapon.special 的池子）：标记 + 每层一个增伤效果（DLL 叠层、扣血）
CRUSH_TIERS = [int(round(40 * 1.18 ** k)) for k in range(24)]          # 40 ~ 1800
# 碾压的实时伤害（用户 2026-11-12「走实时计算伤害的路线」）：同样的冲击波另建这么多套专用行，dark_core 每次放之前把这次的伤害
# （右手武器攻击力 × 百分比）和削韧（同档位的算法 15 + 伤害 / 15）写进攻击行，轮流用；碾压不再挑档（档位留给反震 / 坠星 / 壁垒）
CRUSH_LIVE_N = 2
# 积木引擎的物理冲击波（反震 / 坠星，用户 2026-11-12「反震坠星壁垒也一起改成实时伤害」）另外一批专用行，和碾压的分开（两边各自轮流用，不会互相改到）
SHOCK_LIVE_N = 4
# 法球那套：同上再往上 6 档（到约 4900；迸发越多越痛 2026-11-05，满配一颗约 2950）
# 2026-11-06 法球改乘法器面板（÷ 200）后，后期满层一颗能超过原来的最高档 4860，加到约 1.8 万。
# 同日用户「星群和迸发并没有根据法球数量增加伤害」：原来 18% 一档，每多一颗只涨 7.9%~15%（2.05 / 1.90 最小），相邻颗数常落进同一档；
# 改成 5% 一档，比最小的涨幅小，颗数不同就一定不同档
PEBBLE_TIERS = [int(round(40 * 1.05 ** k)) for k in range(126)]
CRUSH_BULLET, CRUSH_BEH = 920000030, 600003030                         # 魔力爆炸（半径 4、0.2 秒、只打敌人）的子弹和行为
# 冲击波（碾压 / 反震 / 坠星的物理那套、法球的魔力那套）从玩家脚底放：照 505 复制的点位 220 在胸口（离地 1.29 米），
# 900 是脚底（c0000.flver 的 dummy，离地 0，跟随根骨；用户 2026-11-01：冲击波不能在半空）。魔砾是射出去的，照旧从 220
GROUND_DMY = 900
HP_POOL0, HP_POOL_N = 7304100, 64                                       # 精确扣血的专用行（dark_core 运行时写数值，轮流用）
HPMAX_TIERS = 60                                                         # 血瓮：血量上限 +1％ ~ +60％（基础血量 420 以上时 250 点都够）
HP_TIERS = [int(round(5 * 1.18 ** k)) for k in range(55)]              # 挂给敌人的扣血效果：5 ~ 3.8 万（碎冰、瘟疫爆发、巨人杀手；巨人杀手打小怪的 1% 只有个位数；2026-11-06 去掉武器面板上限后，高血量 Boss 的 4% 要放得下）
# 异常累积（挂给敌人就加这么多累积，满了照原版发作）：照原版武器最常用的那行复制，改累积量
BUILDUP_SRC = (('poison', 106000, 'poizonAttackPower'), ('rot', 6603, 'diseaseAttackPower'), ('bleed', 6401, 'bloodAttackPower'),
               ('frost', 107500, 'freezeAttackPower'), ('sleep', 6454, 'sleepAttackPower'), ('madness', 6753, 'madnessAttackPower'))
BUILDUP_AMOUNTS = (60, 80, 100, 150, 200, 750)       # dark_core 挑最接近的一档：积蓄 200、惊醒 750（2026-11-01 加）
# 武器的特殊效果词条位：各稀有度出 0 / 1 个的权重（用户 2026-10-27：武器以后会有更多特效，单独一个词条位去抽）
WEAPON_SPECIAL_COUNT = {'Common': {'0': 100}, 'Rare': {'0': 97, '1': 3}, 'Epic': {'0': 94, '1': 6}, 'Legendary': {'0': 90, '1': 10}}
# 每层伤害 +4%、每次轻击扣 层数×0.5% 最大血量（已经满层的轻击不扣，dark_core runtime.rs）、最多 10 层、5 秒不轻击清零
# （用户 2026-10-31：扣血减半（原来 1%），满层后不再扣血；2026-11-06：统一改成只有轻击才叠层、扣血、续时间）
BLOODRAGE = dict(pct=4, hp=0.5, max=10, reset=5.0,
                 name=('血狂', 'Blood Frenzy'),
                 zh='血狂：轻击时会损失血量，但能提升攻击力；轻击越是连续，提升越多（每层＋4％，最多10层），损失的血量也越多，叠满后不再损失血量；'
                    '受到攻击或5秒不轻击就会消退',
                 en='Blood Frenzy: light attacks cost HP but raise attack power; the longer the chain, the higher it climbs (+4% per stack, up to 10) '
                    'and the more HP it costs, with no HP cost once at full stacks; fades when you are hit or after 5 seconds without a light attack')

# 词条合并与流派权重（用户 2026-10-28：池子太大，远程 / 条件增伤 / 动作增伤不并，其余合并；权重按流派掉率均衡；
# 新的 bd 词条先不做，但流派划分要把规划里的流派算进来）。merge_and_balance 做：
#   - 变体组：组里几条抽词条时算一条（权重平分给组员），掉下来是哪一条随机；名字、说明还是各自的；同一件防具不出同组的两条。
#     旧词条族和效果行都不动，存档里已有的装备照旧（记录存的是效果编号 + 键）。rarity 指定的组员只在这些稀有度出。
#   - 圣杯瓶：红瓶（血量低于一半才生效）+ 蓝瓶合成一条新词条（红蓝都加、不带条件，数值照蓝瓶）；旧的两条权重清零，留给已有装备。
#     卢恩 + 观察力没法合成一行（原版加卢恩的效果都是 stateInfo 76，观察力都是 66），做成变体组。
#   - 权重（用户 2026-10-31：不要按流派，每个词缀位置均匀分布，变体组加起来和其他单条一样）：每条 UNIFORM_W，
#     变体组算一条、组内平分给组员（不看流派）。防具池 / 护符池、每个稀有度分开算，只是哪些能出不同。
#     2026-10-28 前是按流派均衡（通用三类固定份额 GENERIC_SHARE，其余流派平分）；流派现在只是报告里的分类（对不上关键词的算「其他」）。
#     防具上还按部位补偿（rr/affixes.py slot_balanced，变体组也算一条）。
#     防具上「出几个、出在哪个位置」由 settings 的 affix_slots 定（rr/affixes.py → DLL）：前缀位只抽机制类、后缀位只抽数值类。
ARCHETYPES = [                      # (流派, 说明里的关键词)：按顺序找，先命中的算
    ('异常', ['周围发生', '异常累积', '发作']),
    ('命中连击', ['近战攻击命中', '持续以近战攻击', '狂暴地以近战攻击', '近战攻击连续命中', '近战武器命中敌人', '连续攻击的最后一记', '装备重量越轻']),
    ('法术', ['魔法伤害＋', '祷告伤害＋', '咆哮与吐息', '咏唱', '魔法、祷告消耗', '魔法、祷告的有效时间', '魔法、祷告与战技', '魔法与祷告', '法球',
            '蓄力使用魔法、祷告时']),
    ('战技', ['战技伤害', '战技消耗', '蓄力使用魔法', '霸体']),
    ('防御', ['防御时被削减', '防御时，受到', '防御反击', '中断攻击', '壁垒', '破防', '泡状护盾', '相杀']),
    ('机动空战', ['跳跃攻击', '起跳后', '冲刺攻击', '骑马时', '移动速度', '空中']),
    ('闪避', ['翻滚或后跳', '冲撞周围', '所有角色的动作', '见切']),
    ('姿态', ['持握', '共持', '肢体动作', '拉达冈']),
    ('重击处决', ['蓄力攻击', '致命一击', '削减敌人强韧', '踢击', '投掷攻击', '突刺攻击', '满蓄力', '架势', '重击蓄力']),
    ('远程', ['弓与弩', '弓的射程', '道具伤害', '投掷壶', '调香', '弩的装填', '弩箭', '猎印', '投掷道具']),
    ('反击回血', ['打倒敌人时，能恢复血量', '夺回', '失去的血量越多', '饮用', '改为消耗血量', '每失去10％血量', '近战攻击每次命中都能恢复', '血量上限＋#点']),
    ('属性', ['力气＋', '灵巧＋', '智力＋', '信仰＋', '感应＋']),
    ('生存', ['血量上限', '专注值上限', '精力上限', '精力恢复速度', '每#秒恢复', '圣杯瓶', '装备重量上限', '记忆空格', '物理伤害减少', '属性伤害减少', '免疫力']),
    ('杂项', ['打倒敌人后', '血量全满', '血量在80', '灵体死亡', '正面效果', '卢恩', '观察力', '不发出声响', '臭云', '随机获得一种增益', '自动拾取']),
]
# 武器特效的流派（词条百科显示）：说明里没有 ARCHETYPES 关键词的写在这里，其他的照关键词分
SPECIAL_ARCHETYPE = {'bloodrage': '命中连击', 'dr_poise_guard': '防御', 'dr_giant': '命中连击', 'dr_plague': '异常'}
UNIFORM_W = 1200                     # 一条词条的权重（变体组的组员平分；1200 能被 2 / 3 / 4 整除）
VARIANTS = [                        # (组键, 组名, [(组员说明里的关键词, 只在这些稀有度出 / None)])
    ('hit_glintstone', '辉石之力', [('中等辉石之力', None), ('微弱辉石之力', None), ('从天召唤长枪', None), ('魔法之境', None)]),
    ('hit_glintblade', '辉剑圆阵', [('化为守护持用者的辉剑圆阵', ('Common', 'Rare')), ('大型辉剑圆阵', ('Epic', 'Legendary'))]),
    ('hit_fire', '火焰之力', [('巨人之力', None), ('弑神之力', None), ('熔岩强袭', None), ('席欧朵利克的熔岩', None)]),
    ('hit_frost', '冰霜之力', [('冰霜死亡之力', None), ('冰寒范围攻击', None), ('玻列琉斯的冰雾', None)]),
    ('hit_bolt', '雷霆之力', [('积蓄龙之力', None), ('雷龙', None)]),
    ('hit_quake', '震地', [('龙踏', None), ('重力之力', None)]),
    ('hit_wild', '狂野', [('古老野兽', None), ('荆棘', None)]),
    ('hit_heal', '命中回血', [('治疗波', None), ('黄金树', None), ('累积到一定次数时，恢复#％血量', None)]),
    ('near_bleed_summon', '出血召唤', [('旋风护盾', None), ('暗色长枪', None), ('疾风打击', None)]),
    ('near_rot_summon', '毒腐召唤', [('魔力爆炸', None), ('击退旋风', None)]),
    ('near_sleep_madness', '睡眠或发狂', [('周围发生发狂时', None), ('周围发生睡眠时', None)]),
    ('misc_rune_find', '卢恩与观察力', [('获得的卢恩', None), ('观察力', None)]),
]
FLASK = dict(key='dr_flask', src='蓝露滴圣杯瓶的恢复量＋', retire=['蓝露滴圣杯瓶的恢复量＋', '红露滴圣杯瓶的恢复量＋'], suffix=['Flasks', '圣杯'],
             zh='能提升红露滴、蓝露滴圣杯瓶的恢复量（＋%s％）', en='Raises the restoration of Flasks of Crimson and Cerulean Tears by %s%%')

# 护符 / 防具词条分组（和 词条一览.md 一样的判断）
GROUPS = [
    ('属性加点', lambda z: re.match(r'^(力气|灵巧|智力|信仰|感应)＋#$', z)),
    ('魔法流派增伤', lambda z: '魔法' in z and '伤害＋' in z and '魔法、祷告' not in z and '蓄力' not in z),
    ('咆哮与吐息', lambda z: z.startswith('咆哮与吐息')),
    ('祷告流派增伤', lambda z: '祷告' in z and '伤害＋' in z and '魔法、祷告' not in z and '蓄力' not in z and '释放' not in z),
    ('附近触发', lambda z: z.startswith('周围发生')),
    ('命中时触发', lambda z: '命中敌人' in z),
    ('减伤 / 抗性', lambda z: z.startswith('受到敌人的') or z.startswith('免疫力') or z.startswith('防御时')),
    ('资源上限 / 回复', lambda z: any(z.startswith(k) for k in ('血量上限', '专注值上限', '精力上限', '精力恢复', '每#秒恢复', '装备重量上限', '记忆空格', '蓝露滴', '血量低于'))),
    ('施法 / 战技', lambda z: any(k in z for k in ('咏唱', '有效时间', '消耗的专注值', '蓄力使用魔法', '战技伤害'))),
    ('动作增伤', lambda z: any(z.startswith(k) for k in ('蓄力攻击', '冲刺攻击', '跳跃攻击', '翻滚', '致命一击的', '连续攻击', '双手持握', '骑马', '踢击', '防御反击', '突刺攻击', '投掷攻击', '削减敌人'))),
    ('条件增伤', lambda z: any(z.startswith(k) for k in ('血量全满', '打倒敌人后', '致命一击后', '召唤的灵体', '装备重量越轻', '这件物品曾被拉达冈'))),
    ('远程 / 道具', lambda z: any(z.startswith(k) for k in ('弓', '道具伤害', '投掷壶', '调香'))),
    ('其他', lambda z: True),
]
SINGLE_ELEM = ('受到敌人的魔力属性伤害减少#％', '受到敌人的火属性伤害减少#％', '受到敌人的雷属性伤害减少#％', '受到敌人的圣属性伤害减少#％')
ALL_ELEM = '受到敌人的魔力、火、雷、圣属性伤害减少#％'
MERGE = {   # 合并成的新词条：键、说明、后缀（英, 中）、参数里限定流派的标记
    '魔法流派增伤': ('sorcery_dmg', ('Increase damage of Sorceries by #%', '魔法伤害＋#％'), ('Sorcery', '魔法')),
    '祷告流派增伤': ('incantation_dmg', ('Increase damage of Incantations by #%', '祷告伤害＋#％'), ('Incantations', '祷告')),
}
# 武器
STATUS = {'bleed': 'bloodAttackPower', 'poison': 'poizonAttackPower', 'rot': 'diseaseAttackPower', 'frost': 'freezeAttackPower',
          'sleep': 'sleepAttackPower', 'death': 'curseAttackPower', 'madness': 'madnessAttackPower'}
STATUS_TEMPLATE = {'death': 6800, 'madness': 6750}       # 新加的两种异常照原版武器上的这两行（只改累积值）
RESIST = ('changePoisonResistPoint', 'changeDiseaseResistPoint', 'changeBloodResistPoint', 'changeFreezeResistPoint',
          'changeSleepResistPoint', 'changeMadnessResistPoint', 'changeCurseResistPoint')
STATS = ('Strength', 'Agility', 'Magic', 'Faith', 'Luck')
CURVES = ('correctType_Physics', 'correctType_Magic', 'correctType_Fire', 'correctType_Thunder', 'correctType_Dark',
          'correctType_Poison', 'correctType_Blood', 'correctType_Sleep', 'correctType_Madness')
GLOW = {'magic': 7092020, 'fire': 7092030, 'thunder': 7092050, 'holy': 7092000}   # 传说光效（按武器属性；物理不加）
STAFF, SEAL = 57, 61
SHIELDS = (65, 67, 69)     # 小盾、中盾、大盾：只有盾出盾牌抗性（原包里矛、镰刀上也有，不要）
ELEMENT_TABLE = 42300      # 属性补正表：五项属性都能影响五种伤害
# 原包里没有对应护符、找不到流派名字的（DLC 的法杖 / 圣印记），照原版武器说明写
# 卖价中位数（武器 / 防具 / 护符 × 普通 / 稀有 / 史诗 / 传说）：旧版按稀有度拉开后的数（settings 的 sell_value，传说是普通的 2.5 倍）
SELL = {'weapon': [590, 880, 1180, 1470], 'armor': [750, 1120, 1490, 1860], 'talisman': [1530, 2290, 3060, 3820]}
# 法杖 / 圣印记的流派加成统一成这么多（用户 2026-11-13「法杖圣印上各流派伤害加成统一为25%」；原版 10 ~ 30％）。
# 说明按它写；效果行是原版的（原版法杖、圣印记也挂着这些行，只有武器引用），启动时 rr/tweaks.py catalyst_school 按词条库里的 pct 改
SCHOOL_PCT = 25
SCHOOL_NAMES = {16: ('Finger Sorceries', '指头的魔法'), 33: ("Messmer's Fire Incantations", '梅瑟莫之火的祷告'),
                34: ('Golden Ring Incantations', '黄金光环的祷告'), 36: ('Spiral Incantations', '螺旋的祷告')}


def slug(en):
    s = re.sub(r'<[^>]+>', '', en).lower().replace('%', ' pct ').replace('+', ' ')
    s = re.sub(r'[^a-z0-9]+', '_', s).strip('_')
    s = re.sub(r'_+', '_', s)
    return s[:48].rstrip('_')


def lay(name):
    L, size = pdm.layout(pdm.load(os.path.join(RR, 'data', name)))
    return {n: (o, fc) for n, fc, o, *_ in L}, size


def getf(L, row, f):
    o, fc = L[f]
    return struct.unpack_from('<' + fc, row, o)[0]


def setf(L, row, f, v):
    o, fc = L[f]
    struct.pack_into('<' + fc, row, o, v)


def msgs(root, lang, files=('item.msgbnd.dcx', 'item_dlc02.msgbnd.dcx')):
    out = defaultdict(dict)
    for f in files:
        p = os.path.join(root, 'msg', lang, f)
        if os.path.isfile(p):
            for k, v in erlib.read_msgbnd(p).items():
                out[k.replace('_dlc01', '').replace('_dlc02', '')].update(v)
    return out


def head(c):
    return (c or '').split(SEP)[0].strip()


def plain(t):
    return re.sub(r'<[^>]+>', '', t or '').strip()


def hist(c):
    """Counter → 按键排序的 {字符串键: 件数}"""
    return {str(k): v for k, v in sorted(c.items())}


class Extract:
    def __init__(self, pack):
        self.pack = pack
        erlib.DLL_DIRS = [os.path.join(PALY, 'randomizer')]
        self.V = erlib.Bnd4(erlib.read_regulation(os.path.join(VANILLA, 'regulation.bin')))
        self.K = erlib.Bnd4(erlib.read_regulation(os.path.join(pack, 'regulation.bin')))
        self.Pv, self.Pk = {}, {}
        for p in ('SpEffectParam', 'BehaviorParam_PC', 'BehaviorParam', 'Bullet', 'AtkParam_Pc', 'AtkParam_Npc', 'SpEffectVfxParam',
                  'EquipParamAccessory', 'EquipParamProtector', 'EquipParamWeapon', 'EquipParamGem', 'SwordArtsParam',
                  'AttackElementCorrectParam'):
            self.Pv[p] = erlib.Param(self.V.get(p + '.param'))
            self.Pk[p] = erlib.Param(self.K.get(p + '.param'))
        self.SP, _ = lay('SpEffect.xml')
        self.BU, _ = lay('BulletParam.xml')
        self.BE, _ = lay('BehaviorParam.xml')
        self.AT, _ = lay('AtkParam.xml')
        self.WP, _ = lay('EquipParamWeapon.xml')
        self.PR, _ = lay('EquipParamProtector.xml')
        self.AC, _ = lay('EquipParamAccessory.xml')
        self.en, self.zh = msgs(pack, 'engus'), msgs(pack, 'zhocn')
        self.ven, self.vzh = msgs(VANILLA, 'engus'), msgs(VANILLA, 'zhocn')
        self.items = {}                       # (类, 编号) → 稀有度
        self.rar_tag = {}                     # 稀有度 → (英文标签 HTML, 中文标签 HTML)
        for fmg, c in CAT.items():
            for i, t in self.en[fmg].items():
                m = re.search(r'(<font color="#[0-9A-Fa-f]+">(Common|Rare|Epic|Legendary)</font>)', t or '')
                if m:
                    self.items[(c, i)] = m.group(2)
                    mz = re.match(r'^(<font color="#[0-9A-Fa-f]+">[^<]+</font>)', self.zh[fmg].get(i) or '')
                    if mz:
                        self.rar_tag.setdefault(m.group(2), (m.group(1), mz.group(1)))
        self.rows = defaultdict(dict)         # 要插进 regulation 的行：参数名 → {编号: bytes}
        self.report = []
        # 编号锁定（见 pin）：参数名 → {名字: 编号}
        self.lock = json.load(open(LOCK, encoding='utf-8')) if os.path.exists(LOCK) else {}
        self.locked = defaultdict(set, {p: set(d.values()) for p, d in self.lock.items()})
        self.pin_used, self.pin_new = defaultdict(set), []
        self.rule_events = set()              # 积木规则用到的事件（custom / rule_affixes 记，custom_shared 建脉冲标记）
        self.rule_specials = []               # 积木武器特效（rule_special 建，custom_shared 放进特效池）
        self.event_rows = {}                  # 事件 → 词条自己建的效果行（定义里的 events，比如周围出血后 30 秒的状态），不另建脉冲标记
        self.land_markers = set()             # 在听命中事件 land 的词条标记：custom_shared 给它们带上「命中时给对方挂 land 标记」
        self.weapon_land_rules = []           # 听命中的武器特效的规则表：标记不带命中时触发，custom_shared 加一条 while 挂命中检测效果（见 LAND_GEN）

    def sp_row(self, i):
        return self.Pk['SpEffectParam'].rows.get(i)

    # ------------------------------------------------------------ 护符 / 防具
    def gear(self):
        A, Pp = self.Pk['EquipParamAccessory'], self.Pk['EquipParamProtector']
        text, suffix, tal_of = {}, {}, defaultdict(list)
        for i, row in sorted(A.rows.items()):
            if ('a', i) not in self.items:
                continue
            ref = getf(self.AC, row, 'refId')
            m = re.search(r'Amulet of (.+)$', self.en['AccessoryName'].get(i) or '')
            mz = re.match(r'^.*?</font> (.+)护符$', self.zh['AccessoryName'].get(i) or '')
            if ref > 0 and m and mz:
                suffix.setdefault(ref, (m.group(1), mz.group(1)))
                text.setdefault(ref, (head(self.en['AccessoryCaption'].get(i)), head(self.zh['AccessoryCaption'].get(i))))
                tal_of[ref].append(i)
        res = ('residentSpEffectId', 'residentSpEffectId2', 'residentSpEffectId3')
        fam = {}                              # 中文说明模板 → 词条
        cnt_armor, cnt_tal = Counter(), Counter()
        kdist = defaultdict(Counter)
        slots = defaultdict(Counter)
        for i, row in Pp.rows.items():
            if ('p', i) not in self.items:
                continue
            r = self.items[('p', i)]
            ids = [x for x in (getf(self.PR, row, f) for f in res) if x > 0]
            kdist[r][len(ids)] += 1
            slots[r][('head', 'body', 'arm', 'leg')[getf(self.PR, row, 'protectorCategory')]] += 1
            for s in ids:
                if s in text:
                    cnt_armor[s] += 1
        for i, row in A.rows.items():
            if ('a', i) in self.items:
                s = getf(self.AC, row, 'refId')
                if s in text:
                    cnt_tal[s] += 1
        for s, (t_en, t_zh) in text.items():
            z = NUM.sub('#', t_zh)
            r = self.items[('a', tal_of[s][0])]
            f = fam.setdefault(z, dict(zh=z, en=NUM.sub('#', t_en), suffix=list(suffix[s]), sp={}, vals={}, text={}, tal={},
                                       weight={'armor': 0, 'talisman': 0}))
            if r in f['sp'] and f['sp'][r] != s:
                self.report.append('[提示] 同一词条同一稀有度有两个效果：%s %s %d / %d' % (z[:20], r, f['sp'][r], s))
                continue
            f['sp'][r] = s
            f['vals'][r] = NUM.findall(t_zh)
            f['text'][r] = {'en': t_en, 'zh': t_zh}
            f['tal'][r] = tal_of[s][0]
            f['weight']['armor'] += cnt_armor[s]
            f['weight']['talisman'] += cnt_tal[s]
        for f in fam.values():
            f['group'] = next(name for name, test in GROUPS if test(f['zh']))
        self.report.append('原包护符 / 防具词条：%d 种（防具上出现过的 %d 种）' % (len(fam), sum(1 for f in fam.values() if f['weight']['armor'])))

        # ---- 合并
        out = {}
        for name, (key, (t_en, t_zh), suf) in MERGE.items():
            members = [f for f in fam.values() if f['group'] == name]
            merged = dict(zh=t_zh, en=t_en, suffix=list(suf), group=name, sp={}, vals={}, text={}, tal={},
                          weight={k: sum(f['weight'][k] for f in members) for k in ('armor', 'talisman')},
                          merged_from=sorted(f['zh'] for f in members))
            for k, r in enumerate(RAR):
                c = Counter()
                for f in members:
                    if r in f['vals']:
                        c[tuple(f['vals'][r])] += f['weight']['armor'] + f['weight']['talisman']
                if not c:
                    continue
                vals = c.most_common(1)[0][0]
                src = max((f for f in members if tuple(f['vals'].get(r, ())) == vals), key=lambda f: f['weight']['armor'] + f['weight']['talisman'])
                row = bytearray(self.sp_row(src['sp'][r]))
                for fld in ('magicSubCategoryChange1', 'magicSubCategoryChange2', 'magicSubCategoryChange3'):
                    setf(self.SP, row, fld, 0)     # 去掉流派限制：只留「魔法」/「祷告」的标记（和原版大群石雕护符一样）
                sid = self.pin('SpEffectParam', 'sp/%s/%s' % (key, r), lambda: NEW_SP + len(self.rows['SpEffectParam']))
                self.rows['SpEffectParam'][sid] = bytes(row)
                merged['sp'][r] = sid
                merged['vals'][r] = list(vals)
                v = iter(vals)
                merged['text'][r] = {'en': re.sub('#', lambda m: next(v), t_en)}
                v = iter(vals)
                merged['text'][r]['zh'] = re.sub('#', lambda m: next(v), t_zh)
                merged['tal'][r] = ('from', src['tal'][r])
            for f in members:
                del fam[f['zh']]
            out[key] = merged
            self.report.append('合并 %s %d 条 → %s：各稀有度数值 %s（取件数加权最常见的）' % (
                name, len(members), t_zh, ' / '.join(','.join(merged['vals'].get(r, ['-'])) for r in RAR)))
        allf = fam[ALL_ELEM]
        allf['merged_from'] = [ALL_ELEM]
        for z in SINGLE_ELEM:
            f = fam.pop(z)
            for k in ('armor', 'talisman'):
                allf['weight'][k] += f['weight'][k]
            allf['merged_from'].append(z)
        self.report.append('合并 单属性减伤 4 条 → %s（数值照它原来的 %s）' % (ALL_ELEM, ' / '.join(','.join(allf['vals'].get(r, ['-'])) for r in RAR)))
        for z, f in sorted(fam.items(), key=lambda kv: (GROUPS.index(next(g for g in GROUPS if g[0] == kv[1]['group'])), kv[0])):
            key = slug(f['en'])
            while key in out:
                key += '_2'
            out[key] = f

        for key in DROP_GEAR:
            if out.pop(key, None) is None:
                raise RuntimeError('要删的词条 %s 不在了' % key)
        self.report.append('删掉的词条：%s' % ', '.join(DROP_GEAR))

        # ---- 护符模板：每个词条每个稀有度一行，重新编号；合并的词条照来源那件改 refId
        for key, f in out.items():
            for r in RAR:
                if r not in f['tal']:
                    continue
                t = f['tal'][r]
                src = t[1] if isinstance(t, tuple) else t
                row = bytearray(A.rows[src])
                setf(self.AC, row, 'refId', f['sp'][r])
                acc_id = self.pin('EquipParamAccessory', 'tal/%s/%s' % (key, r), self.free_acc)
                self.rows['EquipParamAccessory'][acc_id] = bytes(row)
                f['tal'][r] = acc_id
        self.armor_count = {r: hist(kdist[r]) for r in RAR}
        self.armor_slots = {r: dict(slots[r]) for r in RAR}
        return out

    # ------------------------------------------------------------ 武器
    def weapons(self):
        W, Wv, SPv = self.Pk['EquipParamWeapon'], self.Pv['EquipParamWeapon'], self.Pv['SpEffectParam']
        g = lambda row, f: getf(self.WP, row, f)
        status_of = {}

        def status(sid):
            if sid not in status_of:
                row = SPv.rows.get(sid)
                st = None
                if row is not None and getf(self.SP, row, 'wepParamChange') == 3:
                    hit = [(k, getf(self.SP, row, f)) for k, f in STATUS.items() if getf(self.SP, row, f) > 0]
                    st = hit[0] if len(hit) == 1 else None
                status_of[sid] = st
            return status_of[sid]

        types = defaultdict(Counter)
        ncor = defaultdict(Counter)
        cval = defaultdict(lambda: defaultdict(Counter))
        creq = defaultdict(lambda: defaultdict(Counter))
        curve = defaultdict(lambda: defaultdict(Counter))
        st_n = defaultdict(Counter)
        st_pow = defaultdict(lambda: defaultdict(Counter))
        st_sp = defaultdict(dict)
        res_cnt = Counter()
        res_types = defaultdict(Counter)
        res_of = []                            # (武器类型, 稀有度, [常驻效果])：分完类再数每件保留下来几条
        skills = defaultdict(Counter)
        for i, row in W.rows.items():
            r = self.items.get(('w', i))
            if r is None:
                continue
            t = g(row, 'wepType')
            types[r][t] += 1
            cs = [g(row, 'correct' + s) for s in STATS]
            n = sum(1 for c in cs if c > 0)
            ncor[r][n] += 1
            for k, s in enumerate(STATS):
                if cs[k] > 0:
                    cval[r][str(n)][round(cs[k], 2)] += 1
                    creq[r][s][g(row, 'proper' + s)] += 1
            for f in CURVES:
                curve[r][f[len('correctType_'):]][g(row, f)] += 1
            kinds = set()
            for f in ('spEffectBehaviorId0', 'spEffectBehaviorId1', 'spEffectBehaviorId2'):
                s = g(row, f)
                st = status(s) if s > 0 else None
                if st:
                    kinds.add(st[0])
                    st_pow[st[0]][r][st[1]] += 1
                    st_sp[st[0]][st[1]] = s
            st_n[r][len(kinds)] += 1
            keep = []
            for f in ('residentSpEffectId', 'residentSpEffectId1'):
                s = g(row, f)
                if s > 0:
                    res_cnt[s] += 1
                    res_types[s][t] += 1
                    keep.append(s)
            res_of.append((t, r, keep))
            skills[t][g(row, 'swordArtsParamId')] += 1

        # ---- 命中异常：原包 5 种的强度分布，咒死 / 发狂按同稀有度其他 5 种合起来的分布，新强度的效果照原版模板新建
        pooled = defaultdict(Counter)
        for k in st_pow:
            for r, c in st_pow[k].items():
                pooled[r].update(c)
        status_out = {'count': {r: hist(st_n[r]) for r in RAR}, 'power': {}, 'sp': {}}
        for k in STATUS:
            dist = st_pow[k] if k in st_pow else pooled
            status_out['power'][k] = {r: hist(dist[r]) for r in RAR}
            need = sorted({p for r in RAR for p in dist[r]})
            sps = {}
            for p in need:
                if k in st_sp and p in st_sp[k]:
                    sps[p] = st_sp[k][p]
                    continue
                tpl = STATUS_TEMPLATE[k]
                same = [i for i in range(tpl, tpl + 10) if i in SPv.rows and getf(self.SP, SPv.rows[i], STATUS[k]) == p and
                        self._same_but(SPv.rows[i], SPv.rows[tpl], STATUS[k])]
                if same:
                    sps[p] = same[0]
                    continue
                row = bytearray(SPv.rows[tpl])
                setf(self.SP, row, STATUS[k], p)
                sid = self.pin('SpEffectParam', 'status/%s/%s' % (k, p), lambda: NEW_SP + len(self.rows['SpEffectParam']))
                self.rows['SpEffectParam'][sid] = bytes(row)
                sps[p] = sid
            status_out['sp'][k] = {str(p): s for p, s in sorted(sps.items())}
        self.report.append('武器命中异常：每件几种 %s；新加的咒死 / 发狂按同稀有度其他异常的强度（%s）' % (
            '；'.join('%s %s' % (r, status_out['count'][r]) for r in RAR),
            '，'.join('%s %d~%d' % (k, min(map(int, status_out['sp'][k])), max(map(int, status_out['sp'][k]))) for k in ('death', 'madness'))))

        # ---- 常驻效果：法杖 / 圣印记流派加成（按魔法 / 祷告标记分），盾牌抗性合并成一条
        sch_staff, sch_seal, shield_ids, dropped = Counter(), Counter(), Counter(), Counter()
        for s, n in res_cnt.items():
            row = SPv.rows.get(s)
            if row is None:
                dropped[s] += n
                continue
            if any(getf(self.SP, row, f) for f in RESIST):
                shield_ids[s] += n
            elif getf(self.SP, row, 'magicSubCategoryChange1') and getf(self.SP, row, 'magParamChange'):
                sch_staff[s] += n
            elif getf(self.SP, row, 'magicSubCategoryChange1') and getf(self.SP, row, 'miracleParamChange'):
                sch_seal[s] += n
            else:
                dropped[s] += n
        vals = Counter()
        for s, n in shield_ids.items():
            for v in {getf(self.SP, SPv.rows[s], f) for f in RESIST} - {0}:
                vals[v] += n                   # 和护符 / 防具的合并一样：按件数加权取最常见的那档
        resist_v = vals.most_common(1)[0][0]
        tpl = max(shield_ids, key=lambda s: shield_ids[s])
        row = bytearray(SPv.rows[tpl])
        for f in RESIST:
            setf(self.SP, row, f, resist_v)
        resist_sid = self.pin('SpEffectParam', 'resident/shield_resist', lambda: NEW_SP + len(self.rows['SpEffectParam']))
        self.rows['SpEffectParam'][resist_sid] = bytes(row)
        shield_types = list(SHIELDS)
        cat_n = {}
        for name, ts, pool in (('staff', (STAFF,), sch_staff), ('seal', (SEAL,), sch_seal), ('shield', shield_types, shield_ids)):
            c = defaultdict(Counter)
            for t, r, keep in res_of:
                if t in ts:
                    n = sum(1 for s in keep if s in pool)
                    c[r][min(n, 1) if name == 'shield' else n] += 1      # 盾牌合并成一条：有就是 1
            cat_n[name] = {r: hist(c[r]) for r in RAR}
        school_text = self.school_text()
        resident = {
            'staff': {'types': [STAFF], 'count': cat_n['staff'], 'pool': self.school_pool(sch_staff, school_text)},
            'seal': {'types': [SEAL], 'count': cat_n['seal'], 'pool': self.school_pool(sch_seal, school_text)},
            'shield': {'types': shield_types, 'count': {r: {k: v for k, v in d.items()} for r, d in cat_n['shield'].items()},
                       'pool': [{'sp': resist_sid, 'weight': sum(shield_ids.values()),
                                 'en': 'Immunity, Robustness, Focus and Vitality +%d' % resist_v,
                                 'zh': '免疫力、健壮度、理智度、抗死度＋%d' % resist_v}],
                       'merged_from': sorted(shield_ids)},
        }
        self.report.append('武器常驻：法杖流派加成 %d 种、圣印记 %d 种；盾牌抗性 %d 条合并成一条（全部异常抗性＋%d，%d 种武器类型）；删掉 %d 种（%d 件）' % (
            len(sch_staff), len(sch_seal), len(shield_ids), resist_v, len(shield_types), len(dropped), sum(dropped.values())))

        # ---- 战技：各武器类型能抽的战技（用户 2026-11-04「重新改 按原来同类武器所有可用战灰均匀分布抽 包含专属战技」：以前按原包里出现的次数，
        # 现在 = 原版这类武器能装的战灰（EquipParamGem 的 canMountWep_*，MOUNT_BIT）+ 原版这类武器自己带的战技（含专属），每个一份）；
        # 专属战技记原武器的行为变体，名字按 textId 查
        GE, _ = lay('EquipParamGem.xml')
        gems = {getf(GE, row, 'swordArtsParamId') for row in self.Pv['EquipParamGem'].rows.values()}
        SA = self.Pv['SwordArtsParam']
        names = {}
        for k in ('ArtsName', 'GemName'):
            names.update(self.vzh.get(k, {}))
        native = defaultdict(set)
        type_default = defaultdict(Counter)
        for i, row in Wv.rows.items():
            if i >= 100000000 or i % 10000:
                continue
            s = g(row, 'swordArtsParamId')
            native[s].add(g(row, 'behaviorVariationId'))
            if s in gems:
                type_default[g(row, 'wepType')][s] += 1
        off = GE['canMountWep_Dagger'][0]
        mount = defaultdict(set)                        # 战技 → 能装的武器类型位（原版战灰；sortId 999999 的是 108 行测试用的、什么都能装，不算）
        for row in self.Pv['EquipParamGem'].rows.values():
            if getf(GE, row, 'sortId') == 999999:
                continue
            b = bytes(row)[off:off + 8]
            mount[getf(GE, row, 'swordArtsParamId')] |= {k for k in range(64) if b[k // 8] >> (k % 8) & 1}
        own = defaultdict(set)                          # 武器类型 → 原版这类本体自己带的战技
        for i, row in Wv.rows.items():
            if i < 100000000 and i % 10000 == 0:
                own[g(row, 'wepType')].add(g(row, 'swordArtsParamId'))
        orig_n = {t: (len(c), sum(c.values())) for t, c in skills.items()}
        orig = {t: Counter(c) for t, c in skills.items()}   # 原包的：只用来补 skill_info（已经拿到的武器配方里记着的战技要认得名字）
        for t in list(skills):                          # 原包里有的武器类型才有表（rr/affixes.py 也只写有本体的类型）
            pool = set(own[t]) | ({x for x, bits in mount.items() if MOUNT_BIT[t] in bits} if t in MOUNT_BIT else set())
            skills[t] = Counter({x: 1 for x in pool if x > 0})
        info, bad = {}, Counter()
        for t, c in list(skills.items()) + list(orig.items()):
            for s in list(c):
                if s not in SA.rows:
                    bad[s] += c.pop(s)
                    continue
                tid = struct.unpack_from('<i', SA.rows[s], 12)[0]
                if not names.get(tid):
                    bad[s] += c.pop(s)
                    continue
                if s not in info:
                    vs = sorted(native[s])
                    info[s] = {'gem': s in gems, 'text_id': tid, 'zh': plain(names.get(tid)),
                               'native_var': vs[0] if (s not in gems and len(vs) == 1) else None}
        self.report.append('战技：%d 种（专属 %d 种）；原包里战技不存在或没名字的 %d 件，按类型默认战灰补' % (
            len(info), sum(1 for x in info.values() if not x['gem']), sum(bad.values())))
        self.report.append('战技表（原版能装的战灰 + 原版同类武器自带的战技，均匀）：%s' % '；'.join(
            '%d 类 %d 种（专属 %d；原包 %d 种）' % (t, len(c), sum(1 for x in c if x in info and not info[x]['gem']), orig_n[t][0])
            for t, c in sorted(skills.items())))

        # ---- 传说光效
        for s in GLOW.values():
            self.rows['SpEffectParam'][s] = bytes(self.sp_row(s))
        return {
            'types': {r: hist(types[r]) for r in RAR},
            'scaling': {'count': {r: hist(ncor[r]) for r in RAR},
                        'value': {r: {n: hist(c) for n, c in sorted(cval[r].items())} for r in RAR},
                        'require': {r: {s: hist(creq[r][s]) for s in STATS} for r in RAR},
                        'curve': {r: {k: hist(c) for k, c in curve[r].items()} for r in RAR},
                        'element_table': ELEMENT_TABLE},
            'status': status_out,
            'resident': resident,
            'skills': {str(t): hist(c) for t, c in sorted(skills.items())},
            'skill_default': {str(t): hist(c) for t, c in sorted(type_default.items())},
            'skill_info': {str(s): v for s, v in sorted(info.items())},
            'glow': GLOW,
        }

    def _same_but(self, a, b, field):
        """两行除了 field 都一样"""
        a, b = bytearray(a), bytearray(b)
        setf(self.SP, a, field, 0)
        setf(self.SP, b, field, 0)
        return a == b

    def school_text(self):
        """魔法 / 祷告的流派（magicSubCategory）→ 流派名字，从原包里限定流派的护符说明里取"""
        out = dict(SCHOOL_NAMES)
        for i, row in self.Pk['SpEffectParam'].rows.items():
            sub = getf(self.SP, row, 'magicSubCategoryChange1')
            if not sub or sub in out:
                continue
            acc = [a for a, r in self.Pk['EquipParamAccessory'].rows.items() if ('a', a) in self.items and getf(self.AC, r, 'refId') == i]
            if acc:
                zh = head(self.zh['AccessoryCaption'].get(acc[0]))
                en = head(self.en['AccessoryCaption'].get(acc[0]))
                m, me = re.match(r'^(.*?)伤害＋', zh), re.match(r'^Increase damage of (.+?) by ', en)
                if m and me:
                    out[sub] = (me.group(1).strip(), m.group(1))
        return out

    def school_pool(self, c, school_text):
        out = []
        for s, n in c.most_common():
            row = self.Pv['SpEffectParam'].rows[s]
            sub = getf(self.SP, row, 'magicSubCategoryChange1')
            pct = SCHOOL_PCT
            en, zh = school_text.get(sub, ('', ''))
            out.append({'sp': s, 'weight': n, 'sub': sub, 'pct': pct,
                        'en': ('Increase damage of %s by %d%%' % (en, pct)) if en else '', 'zh': ('%s伤害＋%d％' % (zh, pct)) if zh else ''})
            if not zh:
                self.report.append('[提示] 流派加成 %d（流派 %d）没找到流派名字，说明句要手写' % (s, sub))
        return out

    # ------------------------------------------------------------ 触发类词条：补齐稀有度、按稀有度分伤害、命中触发去冷却
    def row_of(self, p, i):
        r = self.rows[p].get(i)
        if r is None:
            r = self.Pv[p].rows.get(i)
        return None if r is None else bytearray(r)

    def new_id(self, p, start):
        i = self.next_id.get(p, start)
        while i in self.rows[p] or i in self.Pv[p].rows or i in self.Pk[p].rows or i in self.locked[p]:
            i += 10 if p == 'BehaviorParam_PC' else 1
        self.next_id[p] = i + (10 if p == 'BehaviorParam_PC' else 1)
        return i

    def pin(self, p, name, default):
        """装备记录 / 存档里会存的编号（词条族各稀有度的效果、护符模板、武器的异常 / 常驻效果 / 特效）按名字锁定（LOCK）：
        锁过的照锁定的给；没锁过的用 default() 分配并记下。其他内部行（效果链、子弹、行为……）每次重随整体重建，不锁，
        但 new_id 会跳开所有锁定的号。删掉的词条名字留在锁定文件里，号不再分给别人"""
        if name in self.pin_used[p]:
            raise RuntimeError('编号锁定：%s 的名字重复：%s' % (p, name))
        self.pin_used[p].add(name)
        d = self.lock.setdefault(p, {})
        if name in d:
            if d[name] in self.rows[p]:
                raise RuntimeError('编号锁定：%s 的 %s（%d）已经被别的行占了' % (p, name, d[name]))
            return d[name]
        i = default()
        if i in self.rows[p] or i in self.locked[p] or i in self.Pv[p].rows:
            i = self.new_id(p, i)
        d[name] = i
        self.locked[p].add(i)
        self.pin_new.append('%s %s → %d' % (p, name, i))
        return i

    def free_acc(self):
        """护符模板：NEW_ACC 起第一个空号（不看原包，原包的 RNG 护符号段不再存在）"""
        i = NEW_ACC
        while i in self.rows['EquipParamAccessory'] or i in self.Pv['EquipParamAccessory'].rows or i in self.locked['EquipParamAccessory']:
            i += 1
        return i

    def save_lock(self):
        out = {p: dict(sorted(d.items(), key=lambda kv: kv[1])) for p, d in sorted(self.lock.items())}
        json.dump(out, open(LOCK, 'w', encoding='utf-8', newline='\n'), ensure_ascii=False, indent=1)
        idle = {p: sorted(set(d) - self.pin_used[p]) for p, d in self.lock.items()}
        self.report.append('编号锁定（%s）：%d 个名字，这次新锁 %d 个%s；没用到的（删掉的词条，号保留）：%s' % (
            os.path.basename(LOCK), sum(len(d) for d in self.lock.values()), len(self.pin_new),
            '（%s）' % '；'.join(self.pin_new[:12]) + ('…' if len(self.pin_new) > 12 else '') if self.pin_new else '',
            {p: v for p, v in idle.items() if v} or '无'))

    def tier_triggers(self, gear):
        """命中时 / 附近触发里原包只有部分稀有度的词条（原包里各稀有度数值一样）：
        以已有的最高那档为准，四档各一条效果链：伤害（子弹的攻击行）、子弹挂的治疗量、增伤按 TIER_DMG 的比例；
        命中触发的叶子效果（挂着期间不会再触发）持续时间改成效果本身放完要的时间（子弹树估算，不超过原冷却；
        子弹有无限寿命的估不出，照旧），原来「N 秒内每隔几秒放一次」的本来就是效果时长，不动。
        说明：冷却那句改成「效果结束前不会再次触发」，加一行具体伤害 / 回复量。"""
        self.next_id = {}
        SPR = ('accumuOverFireId', 'cycleOccurrenceSpEffectId', 'replaceSpEffectId')
        BU_SP = [n for n in self.BU if n.startswith('spEffectId')]
        RATES = [n for n in self.SP if n.startswith('atkEnemyDmgCorrectRate_') or n.startswith('atkPlayerDmgCorrectRate_')]
        acc_rows = self.rows['EquipParamAccessory']
        sell = {r: Counter(getf(self.AC, row, 'sellValue') for row in acc_rows.values() if RAR[getf(self.AC, row, 'rarity')] == r).most_common(1)[0][0]
                for r in RAR}
        groups = {getf(self.AC, row, 'accessoryGroup') for row in acc_rows.values()}
        n_fam, n_new = 0, 0
        for key, f in gear.items():
            if f['group'] not in ('命中时触发', '附近触发') or len(f['sp']) == len(RAR):
                continue
            orig = [r for r in RAR if r in f['sp']]
            a = orig[-1]
            onhit = f['group'] == '命中时触发'
            info = {'dmg': [], 'heal': [], 'rate': set(), 'inf': False, 'life': 0.0}

            def scan(i, seen):
                """锚点那档的子弹树：伤害行（按出现顺序去重）、治疗量、增伤倍率、估计寿命"""
                if i <= 0 or i in seen:
                    return 0.0
                seen.add(i)
                b = self.row_of('Bullet', i)
                if b is None:
                    return 0.0
                life = getf(self.BU, b, 'life')
                if life < 0:
                    info['inf'] = True
                at = self.row_of('AtkParam_Pc', getf(self.BU, b, 'atkId_Bullet'))
                for fld, _z, _e in ATK_ELEM:
                    v = getf(self.AT, at, fld) if at is not None else 0
                    if v > 0 and (fld, v) not in info['dmg']:
                        info['dmg'].append((fld, v))
                for n in BU_SP:
                    s = self.row_of('SpEffectParam', getf(self.BU, b, n))
                    if s is None:
                        continue
                    hp = getf(self.SP, s, 'changeHpPoint')
                    if hp < 0 and -hp not in info['heal']:
                        info['heal'].append(-hp)
                    info['rate'] |= {round((getf(self.SP, s, x) - 1) * 100, 3) for x in RATES if abs(getf(self.SP, s, x) - 1) > 1e-4}
                return max(life, 0) + max(scan(getf(self.BU, b, 'HitBulletID'), seen), scan(getf(self.BU, b, 'intervalCreateBulletId'), seen))

            def copy_sp_scaled(i, k, memo):
                s = self.row_of('SpEffectParam', i)
                if s is None or (getf(self.SP, s, 'changeHpPoint') >= 0 and all(abs(getf(self.SP, s, x) - 1) < 1e-4 for x in RATES)):
                    return i
                if ('sp', i, k) not in memo:
                    if getf(self.SP, s, 'changeHpPoint') < 0:
                        setf(self.SP, s, 'changeHpPoint', int(round(getf(self.SP, s, 'changeHpPoint') * k)))
                    for x in RATES:
                        v = getf(self.SP, s, x)
                        if abs(v - 1) > 1e-4:
                            setf(self.SP, s, x, 1 + (v - 1) * k)
                    nid = self.new_id('SpEffectParam', TIER_SP)
                    self.rows['SpEffectParam'][nid] = bytes(s)
                    memo[('sp', i, k)] = nid
                return memo[('sp', i, k)]

            def copy_bullet(i, k, memo):
                if i <= 0:
                    return i
                if ('bu', i, k) in memo:
                    return memo[('bu', i, k)]
                b = self.row_of('Bullet', i)
                if b is None:
                    return i
                nid = self.new_id('Bullet', TIER_BULLET)
                memo[('bu', i, k)] = nid
                ai = getf(self.BU, b, 'atkId_Bullet')
                at = self.row_of('AtkParam_Pc', ai) if ai > 0 else None
                if at is not None and any(getf(self.AT, at, fld) > 0 for fld, _z, _e in ATK_ELEM):
                    if ('at', ai, k) not in memo:
                        for fld, _z, _e in ATK_ELEM:
                            setf(self.AT, at, fld, int(round(getf(self.AT, at, fld) * k)))
                        memo[('at', ai, k)] = self.new_id('AtkParam_Pc', TIER_ATK)
                        self.rows['AtkParam_Pc'][memo[('at', ai, k)]] = bytes(at)
                    setf(self.BU, b, 'atkId_Bullet', memo[('at', ai, k)])
                for n in ('HitBulletID', 'intervalCreateBulletId'):
                    setf(self.BU, b, n, copy_bullet(getf(self.BU, b, n), k, memo))
                for n in BU_SP:
                    if getf(self.BU, b, n) > 0:
                        setf(self.BU, b, n, copy_sp_scaled(getf(self.BU, b, n), k, memo))
                self.rows['Bullet'][nid] = bytes(b)
                return nid

            # 锚点那档的效果链（只有词条自己新加的特效行；叶子 = 带行为的）
            chain, todo = [], [f['sp'][a]]
            while todo:
                i = todo.pop(0)
                if i <= 0 or i in chain or i not in self.rows['SpEffectParam']:
                    continue
                chain.append(i)
                todo += [getf(self.SP, self.rows['SpEffectParam'][i], x) for x in SPR]
            leaves = [i for i in chain if getf(self.SP, self.rows['SpEffectParam'][i], 'behaviorId') > 0]
            if len(leaves) != 1:
                raise RuntimeError('触发类词条 %s 的效果链里行为特效不是一个：%s' % (key, leaves))
            beh = getf(self.SP, self.rows['SpEffectParam'][leaves[0]], 'behaviorId')
            brow = self.row_of('BehaviorParam_PC', beh)
            if getf(self.BE, brow, 'refType') != 1:
                raise RuntimeError('触发类词条 %s 的行为不是子弹' % key)
            info['life'] = scan(getf(self.BE, brow, 'refId'), set())
            if not info['dmg'] and not info['heal'] and not info['rate']:
                raise RuntimeError('触发类词条 %s 找不到伤害 / 治疗 / 增伤' % key)
            lrow = self.rows['SpEffectParam'][leaves[0]]
            end0, itv0 = getf(self.SP, lrow, 'effectEndurance'), getf(self.SP, lrow, 'motionInterval')
            end1 = end0
            if onhit and itv0 > end0 and not info['inf']:
                end1 = min(end0, max(1.0, -(-info['life'] * 2 // 1) / 2))

            if key in EN_FIX:
                en = f['text'][a]['en'].split('\n')
                nums = iter(NUM.findall(f['text'][a]['zh'].split('\n')[0]))
                en[0] = re.sub('#', lambda m: next(nums), EN_FIX[key])
                f['text'][a]['en'] = '\n'.join(en)
            src_tal = acc_rows[f['tal'][a]]
            texts = {}
            for r in RAR:
                k = TIER_DMG[r] / TIER_DMG[a]
                memo = {}
                # 链头是这档的词条效果（装备记录里存），锁定；链里其他行内部用
                remap = {i: (i if r == a else self.pin('SpEffectParam', 'sp/%s/%s' % (key, r), lambda: self.new_id('SpEffectParam', TIER_SP))
                             if i == f['sp'][a] else self.new_id('SpEffectParam', TIER_SP)) for i in chain}
                for i in chain:
                    s = bytearray(self.rows['SpEffectParam'][i])
                    for x in SPR:
                        if getf(self.SP, s, x) in remap:
                            setf(self.SP, s, x, remap[getf(self.SP, s, x)])
                    st = getf(self.SP, s, 'stateInfo')
                    if ONHIT_STATE <= st <= ONHIT_STATE + 3:
                        setf(self.SP, s, 'stateInfo', ONHIT_STATE + RAR.index(r))
                    if i == leaves[0]:
                        if end1 != end0:
                            setf(self.SP, s, 'effectEndurance', end1)
                            setf(self.SP, s, 'motionInterval', end1 + 1)
                        if r != a:
                            b = bytearray(brow)
                            setf(self.BE, b, 'refId', copy_bullet(getf(self.BE, brow, 'refId'), k, memo))
                            bid = self.new_id('BehaviorParam_PC', TIER_BEH)
                            self.rows['BehaviorParam_PC'][bid] = bytes(b)
                            setf(self.SP, s, 'behaviorId', bid)
                    self.rows['SpEffectParam'][remap[i]] = bytes(s)
                top = remap[f['sp'][a]]
                # 护符模板：已有的这档改指向新链（旧链的特效和行为删掉；子弹和锚点那档共用，不删），没有的照锚点那件复制
                if r in f['tal']:
                    t = bytearray(acc_rows[f['tal'][r]])
                    old = [f['sp'][r]] if r != a else []
                    while old:
                        i = old.pop()
                        s = self.rows['SpEffectParam'].pop(i, None)
                        if s is not None:
                            old += [getf(self.SP, s, x) for x in SPR]
                            self.rows['BehaviorParam_PC'].pop(getf(self.SP, s, 'behaviorId'), None)
                else:
                    t = bytearray(src_tal)
                    setf(self.AC, t, 'rarity', RAR.index(r))
                    setf(self.AC, t, 'sellValue', sell[r])
                    g = TIER_GROUP
                    while g in groups:
                        g += 1
                    groups.add(g)
                    setf(self.AC, t, 'accessoryGroup', g)
                    f['tal'][r] = self.pin('EquipParamAccessory', 'tal/%s/%s' % (key, r), self.free_acc)
                    n_new += 1
                setf(self.AC, t, 'refId', top)
                acc_rows[f['tal'][r]] = bytes(t)
                f['sp'][r] = top
                # 说明
                fmt = lambda v: ('%.1f' % v).rstrip('0').rstrip('.')
                for lang in ('zh', 'en'):
                    lines = f['text'][a][lang].split('\n')
                    lines[0] = NUM.sub(lambda m: fmt(float(m.group()) * k) if round(float(m.group()), 3) in info['rate'] else m.group(), lines[0])
                    for old, new in (CD_LINE[0] if lang == 'zh' else CD_LINE[1],):
                        pat = re.compile(re.escape(old).replace('\\#', r'[-+]?\d+(?:\.\d+)?'))
                        lines = [new if pat.fullmatch(x.strip()) else x for x in lines]
                    z = lang == 'zh'
                    parts = []
                    if info['dmg']:
                        by = defaultdict(list)
                        for fld, v in info['dmg']:
                            by[fld].append(str(int(round(v * k))))
                        parts.append(('伤害：' if z else 'Damage: ') + ('，' if z else ', ').join(
                            '%s %s' % (nz if z else ne, ' / '.join(by[fld])) for fld, nz, ne in ATK_ELEM if fld in by))
                    if info['heal']:
                        parts.append(('回复血量：' if z else 'HP restored: ') + ' / '.join(str(int(round(v * k))) for v in info['heal']))
                    lines[1:1] = parts
                    texts.setdefault(r, {})[lang] = '\n'.join(lines)
            for r in RAR:
                f['text'][r] = texts[r]
                f['vals'][r] = NUM.findall(texts[r]['zh'])
            f['zh'] = NUM.sub('#', texts[a]['zh'])
            f['en'] = NUM.sub('#', texts[a]['en'])
            f['tiered'] = {'anchor': a, 'lockout': [end0, end1] if onhit else None}
            n_fam += 1
            self.report.append('触发类补齐 %s（原有 %s，以 %s 为准%s）：数值 %s' % (
                f['zh'].split('\n')[0][:30], ''.join(x[0] for x in orig), a,
                '，挂着时长 %g → %g 秒' % (end0, end1) if onhit and end1 != end0 else '',
                ' / '.join(','.join(f['vals'][r]) for r in RAR)))
        self.report.append('触发类词条补齐稀有度：%d 种，新护符模板 %d 件' % (n_fam, n_new))

    def rescale(self, gear):
        """RESCALE 里的词条：效果的倍率字段加成部分乘系数（1 + (v - 1) × k），说明里的数字跟着改"""
        fmt = lambda v: ('%.1f' % v).rstrip('0').rstrip('.')
        for key, (field, k) in RESCALE.items():
            f = gear[key]
            for r, s in f['sp'].items():
                row = bytearray(self.rows['SpEffectParam'][s])
                old = getf(self.SP, row, field)
                setf(self.SP, row, field, 1 + (old - 1) * k)
                self.rows['SpEffectParam'][s] = bytes(row)
                pct = round((old - 1) * 100, 3)
                for lang in ('zh', 'en'):
                    f['text'][r][lang] = NUM.sub(lambda m: fmt(float(m.group()) * k) if abs(float(m.group()) - pct) < 1e-3 else m.group(),
                                                 f['text'][r][lang])
                f['vals'][r] = NUM.findall(f['text'][r]['zh'])
            self.report.append('数值调整 %s：%s 加成 ×%g → %s' % (f['zh'], field, k, ' / '.join(','.join(f['vals'][r]) for r in RAR if r in f['vals'])))
        for key, (fields, pcts) in RESET.items():
            f = gear[key]
            for r, s in f['sp'].items():
                if r not in pcts:
                    continue
                row = bytearray(self.rows['SpEffectParam'][s])
                old = round((getf(self.SP, row, fields[0]) - 1) * 100, 3)
                for fld in fields:
                    setf(self.SP, row, fld, 1 + pcts[r] / 100)
                self.rows['SpEffectParam'][s] = bytes(row)
                for lang in ('zh', 'en'):
                    f['text'][r][lang] = NUM.sub(lambda m: fmt(pcts[r]) if abs(float(m.group()) - old) < 1e-3 else m.group(), f['text'][r][lang])
                f['vals'][r] = NUM.findall(f['text'][r]['zh'])
            self.report.append('数值直接定 %s：%s → %s' % (f['zh'], '、'.join(fields), ' / '.join(','.join(f['vals'][r]) for r in RAR if r in f['vals'])))
        for key, (field, vals) in RESET_VAL.items():
            f = gear[key]
            for r, v in vals.items():
                row = bytearray(self.rows['SpEffectParam'][f['sp'][r]])
                old = getf(self.SP, row, field)
                setf(self.SP, row, field, v)
                self.rows['SpEffectParam'][f['sp'][r]] = bytes(row)
                for lang in ('zh', 'en'):
                    f['text'][r][lang] = NUM.sub(lambda m: str(v) if abs(float(m.group()) - old) < 1e-3 else m.group(), f['text'][r][lang])
                f['vals'][r] = NUM.findall(f['text'][r]['zh'])
            self.report.append('数值直接定 %s：%s → %s' % (f['zh'], field, ' / '.join(','.join(f['vals'][r]) for r in RAR if r in f['vals'])))

    def rework(self, gear):
        """REWORK 里的词条：换后缀、倍率字段改回 1、说明换成新的（数值取原说明前面的几个）"""
        for key, (suffix, fields, (en, zh)) in REWORK.items():
            f = gear[key]
            old = f['suffix'][1]
            f['suffix'], f['zh'], f['en'] = list(suffix), zh, en
            for r, s in f['sp'].items():
                row = bytearray(self.rows['SpEffectParam'][s])
                for fld in fields:
                    setf(self.SP, row, fld, 1.0)
                self.rows['SpEffectParam'][s] = bytes(row)
                f['vals'][r] = f['vals'][r][:zh.count('#')]
                fill = lambda t: re.sub('#', lambda m, v=iter(f['vals'][r]): next(v), t)
                f['text'][r] = {'zh': fill(zh), 'en': fill(en)}
            self.report.append('词条改造 %s → %s：%s 改回 1，说明「%s」（%s）' % (
                old, suffix[1], '、'.join(fields), zh, ' / '.join(','.join(f['vals'][r]) for r in RAR if r in f['vals'])))

    def charged_spell(self, gear):
        """满蓄力：见 CHARGED_SPELL（在 rescale 定数值之后；流派在 merge_and_balance 按说明分）"""
        c = CHARGED_SPELL
        f = gear[c['key']]
        for r, s in f['sp'].items():
            row = bytearray(self.rows['SpEffectParam'][s])
            subs = [getf(self.SP, row, 'magicSubCategoryChange%d' % j) for j in (1, 2, 3)]
            if c['keep'] not in subs or c['drop'] not in subs:
                raise RuntimeError('%s：效果 %d 的攻击子类别是 %s，没有 %d / %d' % (f['zh'], s, subs, c['keep'], c['drop']))
            for j, v in enumerate(subs, 1):
                if v == c['drop']:
                    setf(self.SP, row, 'magicSubCategoryChange%d' % j, 0)
            self.rows['SpEffectParam'][s] = bytes(row)
            fill = lambda t: re.sub('#', lambda m, v=iter(f['vals'][r]): next(v), t)
            f['text'][r] = {'zh': fill(c['zh']), 'en': fill(c['en'])}
        f['zh'], f['en'] = c['zh'], c['en']
        self.report.append('词条改造 %s：只认蓄力魔法、祷告（攻击子类别 %d），去掉蓄力战技（%d）（%s）' % (
            f['suffix'][1], c['keep'], c['drop'], ' / '.join(','.join(f['vals'][r]) for r in RAR if r in f['vals'])))

    def crit_rage(self, gear):
        """狂怒突刺：见 CRIT_RAGE（要在 rule_affixes 之后、custom_shared 之前：resolve_logic 记事件，custom_shared 建脉冲标记）"""
        c = CRIT_RAGE
        f = gear[c['key']]
        made = defaultdict(dict)
        for k, r in enumerate(RAR):
            v = c['vals'][k]
            row = bytearray(self.row_of('SpEffectParam', 360400))
            for fld, val in dict(NEUTRAL_FIELDS, iconId=-1).items():
                setf(self.SP, row, fld, val)
            self.rows['SpEffectParam'][f['sp'][r]] = bytes(row)
            # 图标、光效照原版 20382302；法术也吃（同它的 magParamChange 等）
            made['buff'][r] = self.spec_row(dict(iconId=20318, vfxId=320500, effectEndurance=float(c['secs']), magParamChange=1, miracleParamChange=1,
                                                 shamanParamChange=1, **{x: 1 + v / 100 for x in ATK_RATES}))
            f['vals'][r] = [str(c['secs']), str(v)]
            fill = lambda t: t.replace('#', str(c['secs']), 1).replace('#', str(v), 1)
            f['text'][r] = {'zh': fill(c['zh']), 'en': fill(c['en'])}
        f['zh'], f['en'] = c['zh'], c['en']
        rules = self.resolve_logic(dict(vals=c['vals'], logic=[dict(type='on', event='crit', when='end', effect='buff')]), made)
        f['runtime'] = {'rules': rules, 'vals': list(c['vals'])}
        self.report.append('词条改造 %s：%s（%s）' % (f['suffix'][1], c['zh'].split('\n')[0], ' / '.join(map(str, c['vals']))))

    def high_hp(self, gear):
        """强韧力气：HIGH_HP 的血量条件和说明（效果行编号、数值不变）"""
        c = HIGH_HP
        f = gear[c['key']]
        for r, s in f['sp'].items():
            row = bytearray(self.rows['SpEffectParam'][s])
            setf(self.SP, row, 'conditionHpRate', c['rate'])
            self.rows['SpEffectParam'][s] = bytes(row)
            fill = lambda t: t.replace('#', f['vals'][r][0])
            f['text'][r] = {'zh': fill(c['zh']), 'en': fill(c['en'])}
        f['zh'], f['en'] = c['zh'], c['en']
        self.report.append('词条改造 %s：%s' % (f['suffix'][1], c['zh']))

    def load_speed(self, gear):
        """致命舞（用户 2026-11-06「致命舞越轻动作越快 最高+30%」）：原包词条，效果行换成空标记（编号不变，已有的装备跟着变），dll\\dark_core 的 runtime.rs 做"""
        c = LOAD_SPEED
        f = gear[c['key']]
        for k, r in enumerate(RAR):
            row = bytearray(self.row_of('SpEffectParam', 360400))
            for fld, v in dict(NEUTRAL_FIELDS, iconId=-1).items():
                setf(self.SP, row, fld, v)
            self.rows['SpEffectParam'][f['sp'][r]] = bytes(row)
            f['vals'][r] = [str(c['vals'][k])]
            fill = lambda t: t.replace('#', str(c['vals'][k]))
            f['text'][r] = {'zh': fill(c['zh']), 'en': fill(c['en'])}
        f['zh'], f['en'] = c['zh'], c['en']
        f['runtime'] = {'run': 'loadspeed', 'vals': list(c['vals']), 'sp': {r: [] for r in RAR}}
        self.report.append('词条改造 %s：%s（%s）' % (f['suffix'][1], c['zh'].split('\n')[0], ' / '.join(map(str, c['vals']))))

    def item_dmg(self, gear):
        """ITEM_DMG：留下的那条改成只认 ITEM_CAT 的「道具伤害」；停用的几条效果行换成留下那条同稀有度的、说明一样，标 retired"""
        c = ITEM_DMG
        keep = gear[c['key']]
        olds = []
        for key in (c['key'],) + c['retire']:
            f = gear[key]
            olds.append(f['zh'])
            for r, s in f['sp'].items():
                was = [getf(self.SP, self.rows['SpEffectParam'][s], 'magicSubCategoryChange%d' % j) for j in (1, 2, 3)]
                if was[1:] != [0, 0] or was[0] not in c['cats']:
                    raise RuntimeError('%s：效果 %d 的攻击子类别是 %s，不是 %s 里的一个' % (f['zh'], s, was, c['cats']))
                if f is keep:
                    row = bytearray(self.rows['SpEffectParam'][s])
                    setf(self.SP, row, 'magicSubCategoryChange1', ITEM_CAT)
                    self.rows['SpEffectParam'][s] = bytes(row)
                else:
                    self.rows['SpEffectParam'][s] = self.rows['SpEffectParam'][keep['sp'][r]]
                    f['vals'][r] = list(keep['vals'][r])
                fill = lambda t: re.sub('#', lambda m, v=iter(f['vals'][r]): next(v), t)
                f['text'][r] = {'zh': fill(c['zh']), 'en': fill(c['en'])}
            f['suffix'], f['zh'], f['en'] = list(c['suffix']), c['zh'], c['en']
            if f is not keep:
                f['retired'] = True
        self.report.append('道具伤害统一：%s → %s（攻击子类别 %d，%s）；后两条停用、效果照第一条' % (
            '、'.join(olds), c['zh'], ITEM_CAT, ' / '.join(','.join(keep['vals'][r]) for r in RAR if r in keep['vals'])))

    def hp_drain(self, gear):
        """HP_DRAIN_PCT 里的词条：每秒扣固定血量（changeHpPoint）改成扣最大血量的百分比（changeHpRate），说明跟着改"""
        fmt = lambda v: ('%.1f' % v).rstrip('0').rstrip('.')
        for key, pct in HP_DRAIN_PCT.items():
            f = gear[key]
            olds = []
            for r, s in f['sp'].items():
                d = getf(self.SP, self.rows['SpEffectParam'][s], 'cycleOccurrenceSpEffectId')
                row = bytearray(self.rows['SpEffectParam'][d])
                old = getf(self.SP, row, 'changeHpPoint')
                if old <= 0 or getf(self.SP, row, 'changeHpRate') != 0:
                    raise RuntimeError('%s：效果 %d 不是每秒扣固定血量（changeHpPoint %d）' % (f['zh'], d, old))
                setf(self.SP, row, 'changeHpPoint', 0)
                setf(self.SP, row, 'changeHpRate', pct)
                self.rows['SpEffectParam'][d] = bytes(row)
                olds.append(old)
                t = f['text'][r]
                t['zh'], nz = re.subn(r'每秒损失%d点血量' % old, '每秒损失%s％最大血量' % fmt(pct), t['zh'])
                t['en'], ne = re.subn(r'drain player health by %d points per second' % old, 'drain %s%% of max HP per second' % fmt(pct), t['en'])
                if not (nz and ne):
                    raise RuntimeError('%s：说明里找不到每秒扣 %d 点血的那句' % (f['zh'], old))
                f['vals'][r] = NUM.findall(t['zh'])
            f['zh'] = f['zh'].replace('每秒损失#点血量', '每秒损失#％最大血量')
            f['en'] = f['en'].replace('drain player health by # points per second', 'drain #% of max HP per second')
            self.report.append('持续掉血改百分比 %s：每秒 %s 点 → 每秒 %s％最大血量' % (f['suffix'][1], '/'.join(map(str, olds)), fmt(pct)))

    def rimeguard(self, gear):
        """霜铠：新加的附近触发词条（只出在护符上），判定链和四个稀有度的效果 / 护符模板；返回要接到冻伤发作行上的 hook"""
        def free(ps, start):                  # 几张表里都空着的编号（行为 / 攻击：玩家和敌人两张表用同一个编号）
            i = start
            while any(i in self.rows[p] or i in self.Pv[p].rows or i in self.Pk[p].rows for p in ps):
                i += 1
            return i
        sp = lambda i: bytearray(self.Pv['SpEffectParam'].rows[i])
        bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffect.xml')))[0] if nb}
        emit, mark = self.new_id('SpEffectParam', TIER_SP), self.new_id('SpEffectParam', TIER_SP)
        beh = free(('BehaviorParam_PC', 'BehaviorParam'), TIER_BEH)
        bul = self.new_id('Bullet', TIER_BULLET)
        atk = free(('AtkParam_Pc', 'AtkParam_Npc'), TIER_ATK)
        vfx = free(('SpEffectVfxParam',), 321711)
        r = sp(505)                           # 放判定：瞬间效果，只管放行为
        for fld, v in (('effectEndurance', 0.0), ('motionInterval', 0.0), ('behaviorId', beh),
                       ('invocationConditionsStateChange1', 0), ('invocationConditionsStateChange2', 0)):
            setf(self.SP, r, fld, v)
        o, bp = bits['isWaitModeDelete']
        r[o] &= ~(1 << bp)
        self.rows['SpEffectParam'][emit] = bytes(r)
        r = sp(506)                           # 标记：空效果，只有 stateInfo
        setf(self.SP, r, 'stateInfo', RIME_STATE)
        self.rows['SpEffectParam'][mark] = bytes(r)
        for p in ('BehaviorParam_PC', 'BehaviorParam'):
            r = bytearray(self.Pv[p].rows[2105])
            setf(self.BE, r, 'refId', bul)
            if p == 'BehaviorParam':
                setf(self.BE, r, 'category', 0)
            self.rows[p][beh] = bytes(r)
        r = bytearray(self.Pv['Bullet'].rows[1005])
        setf(self.BU, r, 'atkId_Bullet', atk)
        setf(self.BU, r, 'spEffectId0', mark)
        self.rows['Bullet'][bul] = bytes(r)
        for p in ('AtkParam_Pc', 'AtkParam_Npc'):  # 原版攻击 2 的复制，和腐败那条判定分开
            self.rows[p][atk] = bytes(self.Pv[p].rows[2])
        VX, _ = lay('SpEffectVfx.xml')
        r = bytearray(self.Pv['SpEffectVfxParam'].rows[321700])   # 特效换成冰的（sfx / 音效都是原版的，和 VINS 一样）
        for fld, v in (('midstSfxId', 4500), ('midstSeId', 524402), ('initSfxId', 525083), ('midstDmyId', 905)):
            setf(VX, r, fld, v)
        vb = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffectVfx.xml')))[0] if nb}
        o, bp = vb['isMidstFullbody']
        r[o] &= ~(1 << bp)
        self.rows['SpEffectVfxParam'][vfx] = bytes(r)

        like = gear[RIME_LIKE]
        groups = {getf(self.AC, row, 'accessoryGroup') for row in self.rows['EquipParamAccessory'].values()}
        # sp_more：链头以外还要挂的监听（护符 residentSpEffectId1；value_ranges 的效果链也从这里展开，dark_core 词条数值随机把没被引用的行都当链头挂）
        f = dict(zh=NUM.sub('#', RIME_TEXT['zh']), en=NUM.sub('#', RIME_TEXT['en']), suffix=['Rimeguard', '霜铠'], group='附近触发',
                 sp={}, sp_more={}, vals={}, text={}, tal={}, weight={'armor': 0, 'talisman': RIME_WEIGHT})
        RATES = [n for n in self.SP if n.startswith(('atkEnemyDmgCorrectRate_', 'atkPlayerDmgCorrectRate_'))]
        for k, (r_, (cut, stam)) in enumerate(zip(RAR, RIME_TIERS)):
            listen = self.pin('SpEffectParam', 'sp/%s/%s' % (RIME_KEY, r_), lambda: self.new_id('SpEffectParam', TIER_SP))
            cut_buff = self.new_id('SpEffectParam', TIER_SP)
            # 第二个监听和精力恢复那行放到单独的号段并锁定：不走 new_id 的游标，后面别的内部行的编号不跟着挪
            listen2 = self.pin('SpEffectParam', 'sp2/%s/%s' % (RIME_KEY, r_), lambda: RIME_MORE_SP + 2 * k)
            stam_buff = self.pin('SpEffectParam', 'stam/%s/%s' % (RIME_KEY, r_), lambda: RIME_MORE_SP + 2 * k + 1)
            for lid, bid in ((listen, cut_buff), (listen2, stam_buff)):
                r = sp(321700)                    # 监听：原版这行带腐败眷属的欢愉的图标，摘掉（图标只放在增益上）
                for fld, v in (('cycleOccurrenceSpEffectId', bid), ('invocationConditionsStateChange1', RIME_STATE), ('iconId', -1)):
                    setf(self.SP, r, fld, v)
                self.rows['SpEffectParam'][lid] = bytes(r)
            for bid, cut_, stam_, icon, vx in ((cut_buff, cut, 0, RIME_CUT_ICON, vfx), (stam_buff, 0, stam, RIME_STAM_ICON, -1)):
                r = sp(321701)                    # 特效只挂在减伤那行上（两行一起挂，挂两份就叠了）
                for fld in RATES:
                    setf(self.SP, r, fld, 1.0)
                for fld in CUT_FIELDS:
                    setf(self.SP, r, fld, 1.0 - cut_ / 100.0)
                for fld, v in (('iconId', icon), ('effectEndurance', RIME_SEC), ('vfxId', vx), ('staminaRecoverChangeSpeed', stam_)):
                    setf(self.SP, r, fld, v)
                self.rows['SpEffectParam'][bid] = bytes(r)
            t = bytearray(self.rows['EquipParamAccessory'][like['tal'][r_]])
            if any(getf(self.AC, t, 'residentSpEffectId%d' % j) > 0 for j in range(1, 5)):
                raise RuntimeError('霜铠：护符模板 %d 的常驻效果栏不空，第二个监听放不下' % like['tal'][r_])
            g = TIER_GROUP
            while g in groups:
                g += 1
            groups.add(g)
            tid = self.pin('EquipParamAccessory', 'tal/%s/%s' % (RIME_KEY, r_), self.free_acc)
            for fld, v in (('refId', listen), ('residentSpEffectId1', listen2), ('iconId', RIME_ICON), ('accessoryGroup', g)):
                setf(self.AC, t, fld, v)
            self.rows['EquipParamAccessory'][tid] = bytes(t)
            f['sp'][r_], f['sp_more'][r_], f['tal'][r_] = listen, [listen2], tid
            f['text'][r_] = {'zh': RIME_TEXT['zh'] % (RIME_SEC, cut, stam), 'en': RIME_TEXT['en'] % (cut, stam, RIME_SEC)}
            f['vals'][r_] = NUM.findall(f['text'][r_]['zh'])
        gear[RIME_KEY] = f
        self.report.append('新词条 霜铠（附近冻伤 %g 秒，受到伤害 %s，精力恢复 %s）：效果 %s（第二个监听 %s），护符模板 %s；'
                           '判定 %d → 行为 %d → 子弹 %d（攻击 %d）→ 标记 %d（stateInfo %d），特效 %d；图标 减伤 %d、精力恢复 %d' % (
            RIME_SEC, ' / '.join('-%d%%' % c for c, _ in RIME_TIERS), ' / '.join('+%d' % s for _, s in RIME_TIERS),
            ' / '.join(str(f['sp'][r]) for r in RAR), ' / '.join(str(f['sp_more'][r][0]) for r in RAR),
            ' / '.join(str(f['tal'][r]) for r in RAR), emit, beh, bul, atk, mark, RIME_STATE, vfx, RIME_CUT_ICON, RIME_STAM_ICON))
        self.rime_emit = emit
        return [{'param': 'SpEffectParam', 'where': {'stateInfo': 260, 'spCategory': 10007}, 'field': 'cycleOccurrenceSpEffectId',
                 'empty': [-1, 0], 'value': emit, 'what': '冻伤发作接霜铠的附近判定'}]

    def custom_vfx(self):
        """自制特效的光效行（CUSTOM_VFX：编号 → (特效, 挂点[, effectType[, 起手音效[, 'init']]])）：照原版 1709000（挂给敌人的降攻光效，持续特效、无起手特效）复制，
        只换持续特效和挂点；挂在武器上的（挂点 10000 / 20000 段）照原版武器光效再写 effectType 1 右手 / 2 左手；音效（> 0 才写）= 持续音效，跟持续特效一起放"""
        VX, _ = lay('SpEffectVfx.xml')
        vb = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffectVfx.xml')))[0] if nb}
        for vid, spec in CUSTOM_VFX.items():
            sfx, dmy = spec[:2]
            if vid in self.Pv['SpEffectVfxParam'].rows:
                raise RuntimeError('自制光效行 %d 和原版重了' % vid)
            r = bytearray(self.Pv['SpEffectVfxParam'].rows[VFX_WEAKEN])
            if len(spec) > 4 and spec[4] == 'init':     # 只有开头特效（一次性放完，交替）
                setf(VX, r, 'midstSfxId', -1)
                setf(VX, r, 'midstDmyId', -1)
                setf(VX, r, 'initSfxId', sfx)
                setf(VX, r, 'initDmyId', dmy)
            else:
                setf(VX, r, 'midstSfxId', sfx)
                setf(VX, r, 'midstDmyId', dmy)
            if len(spec) > 2:
                setf(VX, r, 'effectType', spec[2])
                # 写了第三项（effectType，相杀火花）的不进演出类别，同原版武器光效 8575 / 8576（模板 1709000 是类别 4、优先级 30：身体光效那一类，同类同时只放优先级高的，
                # 词条 / 原版增益的身体光效（优先级 10~13）在身上时相杀火花就被压掉，用户 2026-11-04「火花特效非常不明显」「按理说应该很明显了」）
                setf(VX, r, 'playCategory', 0)
                setf(VX, r, 'playPriority', 0)
            if len(spec) > 3 and spec[3] > 0:
                setf(VX, r, 'midstSeId', spec[3])
            o, bp = vb['isMidstFullbody']
            r[o] &= ~(1 << bp)
            self.rows['SpEffectVfxParam'][vid] = bytes(r)

    def custom(self, gear):
        """CUSTOM 里的新词条：标记效果、玩家脚本 / DLL 要挂的效果行、护符模板；词条带 slots（防具部位）和 runtime（谁来做、要挂的效果）"""
        lay_bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffect.xml')))[0] if nb}
        fmt = lambda v: ('%.1f' % v).rstrip('0').rstrip('.')

        def sp_from(src, pin=None, **vals):    # pin：装备记录里会存的（词条的标记效果），按名字锁定编号
            r = bytearray(self.Pv['SpEffectParam'].rows[src])
            for fld, v in vals.items():
                setf(self.SP, r, fld, v)
            new = lambda: self.new_id('SpEffectParam', TIER_SP)
            i = self.pin('SpEffectParam', pin, new) if pin else new()
            self.rows['SpEffectParam'][i] = bytes(r)
            return i

        def marker(pin):
            return sp_from(360400, pin=pin, iconId=-1, fallDamageRate=1.0, spCategory=0)

        def clear_bit(i, name):
            r = bytearray(self.rows['SpEffectParam'][i])
            if name in lay_bits:
                o, bp = lay_bits[name]
                r[o] &= ~(1 << bp)
            else:
                setf(self.SP, r, name, 0)
            self.rows['SpEffectParam'][i] = bytes(r)
        armor_w = sorted(f['weight']['armor'] for f in gear.values() if f['weight']['armor'] > 0)
        armor_w = armor_w[len(armor_w) // 2]
        like = gear[RIME_LIKE]
        groups = {getf(self.AC, row, 'accessoryGroup') for row in self.rows['EquipParamAccessory'].values()}
        for c in CUSTOM:
            tal = 'talisman' in c['slots']
            slots = [s for s in c['slots'] if s in SLOT_BIT]
            f = dict(zh=NUM.sub('#', c['zh']).replace('%s', '#'), en=NUM.sub('#', c['en']).replace('%s', '#').replace('%%', '%'),
                     suffix=list(c['suffix']), group='新词条', sp={}, vals={}, text={}, tal={},
                     weight={'armor': armor_w if slots else 0, 'talisman': RIME_WEIGHT if tal else 0},
                     slots=sum(SLOT_BIT[s] for s in slots), runtime={'run': c['run'], 'vals': list(c['vals']), 'sp': {}})
            made = defaultdict(dict)                    # 定义里也写了 rows / logic 的（比如霸王的图标、光效交给积木引擎）
            for k, r_ in enumerate(RAR):
                v = c['vals'][k]
                self.make_rows(c, r_, v, made)
                run = []
                f['sp'][r_] = marker('sp/%s/%s' % (c['key'], r_))
                if c['run'] == 'leap':                 # 计时标记 v 秒：dll\dark_core 在它挂着时设翻滚同款的无敌位（dodging）
                    run = [sp_from(360400, iconId=-1, fallDamageRate=1.0, spCategory=0, effectEndurance=float(v))]
                elif c['run'] == 'overlord':           # 照「忍耐」1650；减伤按稀有度，0.3 秒、玩家脚本每帧续。
                    # 一次性霸体（用户 2026-10-28）：受击等级替换清掉，挡不挡全由玩家脚本判断（挡下一次后脚本不再挂、dark_core 马上摘掉）。
                    # 图标、光效不放这里（0.3 秒的效果图标会一直闪），交给积木引擎：定义里的 rows['look'] + while armor
                    i = sp_from(1650, iconId=-1, vfxId=-1, effectEndurance=0.3, spCategory=0,
                                **{x: 1.0 - v / 100.0 for x in DEF_RATES}, **{x: 0 for x in DMG_LV})
                    clear_bit(i, 'isContractSpEffectLife')
                    run = [i]
                elif c['run'] == 'elegance':           # 第 n 层：造成的伤害 +n×v%（DLL 按层数换，20 秒）
                    run = [sp_from(360400, iconId=status_icon('elegance', n), vfxId=VFX_ELEGANCE, fallDamageRate=1.0, spCategory=0,
                                   effectEndurance=20.0, **{x: 1.0 + n * v / 100.0 for x in ATK_RATES}) for n in range(1, CUSTOM_STACKS + 1)]
                elif c['run'] == 'cocktail':           # 六种效果，各 10 秒
                    base = dict(fallDamageRate=1.0, spCategory=0, effectEndurance=10.0)
                    for name, icon, vv in COCKTAIL:
                        x = vv[k]
                        extra = {'atk': {y: 1.0 + x / 100.0 for y in ATK_RATES},
                                 'cut': {y: 1.0 - x / 100.0 for y in CUT_FIELDS},
                                 'stam': {'staminaRecoverChangeSpeed': int(x)},
                                 'hp': {'changeHpRate': -float(x), 'motionInterval': 1.0},
                                 'fp': {'changeMpPoint': -int(x), 'motionInterval': 1.0},
                                 'poise': {'saAttackPowerRate': 1.0 + x / 100.0}}[name]
                        run.append(sp_from(360400, iconId=icon, vfxId=COCKTAIL_VFX[name], **base, **extra))
                f['runtime']['sp'][r_] = run
                # 说明
                if c['run'] == 'cocktail':
                    args = tuple(fmt(vv[k]) for _n, _i, vv in COCKTAIL)
                    zh, en = c['zh'] % args, c['en'] % args
                elif v == 0 and 'zh0' in c:
                    zh, en = c['zh0'], c['en0']
                else:
                    zh, en = c['zh'] % fmt(v), c['en'] % fmt(v)
                f['text'][r_] = {'zh': zh, 'en': en}
                f['vals'][r_] = NUM.findall(zh)
                if tal:                                 # 护符模板（照血热的模板，换效果、图标、组）
                    t = bytearray(self.rows['EquipParamAccessory'][like['tal'][r_]])
                    g = TIER_GROUP
                    while g in groups:
                        g += 1
                    groups.add(g)
                    tid = self.pin('EquipParamAccessory', 'tal/%s/%s' % (c['key'], r_), self.free_acc)
                    for fld, val in (('refId', f['sp'][r_]), ('iconId', c['icon']), ('accessoryGroup', g)):
                        setf(self.AC, t, fld, val)
                    self.rows['EquipParamAccessory'][tid] = bytes(t)
                    f['tal'][r_] = tid
            if c.get('logic'):
                f['runtime']['rules'] = self.resolve_logic(c, made)
                self.rule_extras(c, list(f['sp'].values()), made, f['runtime']['rules'])    # 听命中的（混沌）：标记带上命中时触发
            gear[c['key']] = f
            self.report.append('新词条 %s（%s，%s 做）：%s' % (c['suffix'][1], '、'.join(c['slots']), c['run'],
                                                         ' / '.join(f['text'][r]['zh'].split('\n')[0] for r in RAR)))

    def spec_row(self, spec, pin=None):
        """照 spec 建一行效果：src= 底行（默认 360400 空效果，去掉它自己的效果），其余是字段；pin：装备记录里会存的，锁定编号"""
        spec = dict(spec)
        if spec.pop('enemy', False):                    # 挂给敌人的（目标层，dark_core 直接挂）：借 880 的目标位，补上「挂给自己」位
            return self.enemy_sp(direct=True, **spec)
        direct = spec.pop('direct', False)              # dark_core 直接挂到身上的，底子缺「挂给自己」位时要补（见 allow_direct）
        r = bytearray(self.row_of('SpEffectParam', spec.pop('src', 360400)))
        for fld, v in dict(NEUTRAL_FIELDS, **spec).items():
            setf(self.SP, r, fld, v)
        new = lambda: self.new_id('SpEffectParam', TIER_SP)
        i = self.pin('SpEffectParam', pin, new) if pin else new()
        self.rows['SpEffectParam'][i] = bytes(r)
        if direct:
            self.allow_direct(i)
        return i

    def make_rows(self, c, r, v, made):
        """词条定义的 rows 在稀有度 r（数值 v）建出来：made[行名][r] = 编号 / 编号列表（叠层每层一行）"""
        for name, make in (c.get('rows') or {}).items():
            spec = make(v)
            made[name][r] = [self.spec_row(s) for s in spec] if isinstance(spec, list) else self.spec_row(spec)

    def resolve_logic(self, c, made):
        """词条定义的 logic 换成规则：函数按稀有度算出数值，effect / fx 的行名换成各稀有度的编号；用到的事件记进 self.rule_events"""
        rules = []
        for spec in c.get('logic') or []:
            rule = {}
            for k2, v2 in spec.items():
                if callable(v2):                        # 按稀有度的数值
                    rule[k2] = [v2(c['vals'][i]) for i in range(len(RAR))]
                elif k2 in ('effect', 'flash', 'target_fx', 'poise_fx'):
                    rule[k2] = [made[v2].get(r, -1) for r in RAR]
                elif k2 in ('fx', 'mfx', 'tfx'):         # 叠层每层一行 / 目标身上的标记每层一行 / 消耗标记时按层数挑的特效
                    rule[k2] = [(lambda x: x if isinstance(x, list) else [x])(made[v2][r]) if r in made[v2] else [] for r in RAR]
                else:
                    rule[k2] = v2
            for k2 in ('cond', 'unless', 'event', 'prog_from'):
                if k2 in rule:                          # on 的 unless 可以是几个事件
                    self.rule_events.update(rule[k2] if isinstance(rule[k2], list) else [rule[k2]])
            for k2 in ('add', 'clear', 'consume'):
                self.rule_events.update(rule.get(k2, []))
            if rule['type'] == 'free':                  # 无尽箭袋：射击 / 投掷的状态和新的一次（同 rr/affixes.py 的 rule_events）
                self.rule_events.update(FREE_EVENTS)
            if 'haste' in rule or 'act_speed' in rule or 'layer_speed' in rule:  # 改动作速度的：处决、被抓时不变速，要听 sync（同 rr/affixes.py 的 rule_events）
                self.rule_events.add('sync')
            if rule.get('event') == 'mikiri':           # 见切是 dark_core 在翻滚 / 后撤步中判断的（同 rr/affixes.py 的 rule_events）
                self.rule_events.update(('roll', 'backstep'))
            if self.rule_events & {'ground', 'air'}:    # 在地上 / 在空中：玩家脚本在空中时每帧发 air（dark_core 没收到就算在地上）
                self.rule_events.add('air')
            rules.append(rule)
        return rules

    def mark_fields(self, c, marker, made, r, v):
        """定义里的 mark：标记行自己带的字段（值是行名就换成稀有度 r 的编号，是函数就按这档数值 v 算），比如打倒敌人时触发、
        周围发生出血时触发、反扑的虚血倍率"""
        for fld, val in (c.get('mark') or {}).items():
            m = bytearray(self.rows['SpEffectParam'][marker])
            setf(self.SP, m, fld, made[val][r] if isinstance(val, str) else val(v) if callable(val) else val)
            self.rows['SpEffectParam'][marker] = bytes(m)

    def rule_extras(self, c, markers, made, rules):
        """定义里的 events（事件绑到自己建的效果行，各稀有度要是同一行）；在听命中事件 land 的，标记记进 self.land_markers"""
        for ev, name in (c.get('events') or {}).items():
            if isinstance(name, int):                   # 直接用原版的效果（比如蹲下 = 潜行效果 8001）
                self.event_rows[ev] = name
                continue
            ids = set(made[name].values())
            if len(ids) != 1:
                raise RuntimeError('积木词条 %s 的事件 %s 绑的行各稀有度不一样' % (c['key'], ev))
            self.event_rows[ev] = ids.pop()
        if any('land' in [rule.get('cond'), rule.get('event')] + rule.get('add', []) for rule in rules):
            if tuple(c['slots']) == ('weapon',):
                self.weapon_land_rules.append(rules)    # 武器特效：标记挂在武器的常驻效果位，带 152 会让这把武器附不上魔（见 LAND_GEN）
            else:
                self.land_markers.update(m for m in markers if m > 0)

    def enemy_sp(self, direct=False, pin=None, **vals):
        """挂给被打的一方的效果（命中时触发 atkOccurrenceSpEffectId 用）：空效果 360400，只借原版 880 的「挂给谁」位（effectTarget*）。
        880 本身是冻伤发作（状态 260、扣 10%＋30 血、冻伤光效），整行照抄的话每一击都等于让对方冻伤（混沌原来就是这样）。
        direct：dark_core 直接挂给敌人的（积木引擎 FoeApply），再补上「挂给自己」位（见 allow_direct）"""
        bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffect.xml')))[0] if nb}
        r = bytearray(self.row_of('SpEffectParam', 360400))
        src = self.Pv['SpEffectParam'].rows[ENEMY_LIKE]
        for n, (o, bp) in bits.items():
            if n.startswith('effectTarget'):
                r[o] = (r[o] & ~(1 << bp)) | (src[o] & (1 << bp))
        for fld, v in dict(NEUTRAL_FIELDS, **vals).items():
            setf(self.SP, r, fld, v)
        i = pin if pin is not None else self.new_id('SpEffectParam', TIER_SP)
        self.rows['SpEffectParam'][i] = bytes(r)
        if direct:
            self.allow_direct(i)
        return i

    def allow_direct(self, i):
        """dark_core 直接挂给角色的效果行（积木引擎 FoeApply：标记、扣血档、异常累积……）：补上 effectTargetSelf / effectTargetSelfTarget。
        游戏的「给角色挂效果」把被挂的角色当成施放者自己，只有「命中时挂给对方」位的行一律被拒（2026-10-28 游戏里查的：
        挂完立刻读身上没有；原版 1709000 挂给自己也一样，黄金誓约 1730 带这两位就挂得上）。敌人那边的 AI / 生存位 880 本来就有"""
        bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffect.xml')))[0] if nb}
        r = bytearray(self.rows['SpEffectParam'][i])
        for n in DIRECT_TARGET_BITS:
            o, bp = bits[n]
            r[o] |= 1 << bp
        self.rows['SpEffectParam'][i] = bytes(r)

    def rule_special(self, c, rars, fmt):
        """武器特效的积木词条（slots=('weapon',)）：一个标记（写进武器的常驻效果位，各稀有度同一个，锁定编号 special/<键>），
        rarities 限定能出的稀有度；数值取能出的最高那档（特效不分档）。进 self.rule_specials，custom_shared 放进特效池"""
        v = c['vals'][RAR.index(rars[-1])]
        marker = self.spec_row(dict(iconId=-1), pin='special/%s' % c['key'])
        made = defaultdict(dict)
        self.make_rows(c, rars[-1], v, made)
        self.mark_fields(c, marker, made, rars[-1], v)
        for name in list(made):
            made[name] = {r: made[name][rars[-1]] for r in RAR}
        args = tuple(fmt(x) for x in v) if isinstance(v, tuple) else (fmt(v),) * c['zh'].count('%s')
        rules = self.resolve_logic(c, made)
        self.rule_extras(c, [marker], made, rules)
        self.rule_specials.append({
            'key': c['key'], 'sp': marker, 'weight': c.get('weight', 1), 'types': list(c.get('types', [])),
            'name': [c['suffix'][1], c['suffix'][0]], 'zh': '%s：%s' % (c['suffix'][1], c['zh'] % args),
            'en': '%s: %s' % (c['suffix'][0], c['en'] % args), 'rarities': list(rars), 'rules': rules})
        self.report.append('积木武器特效 %s（仅 %s）：%s' % (c['suffix'][1], '/'.join(rars), c['zh'] % args))

    def rule_affixes(self, gear):
        """RULE_AFFIXES 的积木词条：标记效果（锁定编号）、rows 的效果行、logic 换成编号后的规则（runtime.rules）、护符模板。
        用到的事件记在 self.rule_events，custom_shared 给没有的事件建脉冲标记"""
        fmt = lambda v: ('%.1f' % v).rstrip('0').rstrip('.')
        armor_w = sorted(f['weight']['armor'] for f in gear.values() if f['weight']['armor'] > 0)
        armor_w = armor_w[len(armor_w) // 2]
        like = gear[RIME_LIKE]
        groups = {getf(self.AC, row, 'accessoryGroup') for row in self.rows['EquipParamAccessory'].values()}
        pv_groups = {getf(self.AC, row, 'accessoryGroup') for row in self.Pv['EquipParamAccessory'].rows.values()}
        if KEYSTONE_GROUP in groups or KEYSTONE_GROUP in pv_groups:
            raise RuntimeError('规则改变器的护符组 %d 已经有人用了' % KEYSTONE_GROUP)
        for c in RULE_AFFIXES:
            rars = c.get('rarities', RAR)
            if 'weapon' in c['slots']:
                self.rule_special(c, rars, fmt)
                continue
            tal = 'talisman' in c['slots']
            slots = [s for s in c['slots'] if s in SLOT_BIT]
            f = dict(zh=c['zh'].replace('%s', '#'), en=c['en'].replace('%s', '#').replace('%%', '%'), suffix=list(c['suffix']), group='新词条',
                     sp={}, vals={}, text={}, tal={}, weight={'armor': armor_w if slots else 0, 'talisman': RIME_WEIGHT if tal else 0},
                     slots=sum(SLOT_BIT[s] for s in slots), keystone=bool(c.get('keystone')) and not c.get('own_group'))
            made = defaultdict(dict)                    # 行名 → 稀有度 → 编号 / 编号列表
            for k, r_ in enumerate(RAR):
                if r_ not in rars:
                    continue
                v = c['vals'][k]
                f['sp'][r_] = self.spec_row(dict(iconId=-1), pin='sp/%s/%s' % (c['key'], r_))
                self.make_rows(c, r_, v, made)
                self.mark_fields(c, f['sp'][r_], made, r_, v)
                tv = c.get('text_val', lambda x: x)       # 说明里的数字可以换算（血契：500 → 「5倍」）
                args = tuple(fmt(x) for x in tv(v)) if isinstance(v, tuple) else (fmt(tv(v)),) * c['zh'].count('%s')
                # 数值是 0 的稀有度用没有数字的那句（同 CUSTOM 的 zh0；疫病传说不加抗性）
                zero = 'zh0' in c and v == 0
                f['text'][r_] = {'zh': c['zh0'], 'en': c['en0']} if zero else {'zh': c['zh'] % args, 'en': c['en'] % args}
                f['vals'][r_] = NUM.findall(f['text'][r_]['zh'])
                if tal:                                 # 护符模板（照血热的模板，换效果、图标、组；规则改变器共用一个组）
                    t = bytearray(self.rows['EquipParamAccessory'][like['tal'][r_]])
                    if c.get('own_group'):              # 自己一个组：和其他护符（包括别的规则改变器）都不冲突
                        g = OWN_GROUP
                        while g in groups or g in pv_groups:
                            g += 1
                        if g >= TIER_GROUP:
                            raise RuntimeError('单独组的护符太多了（%d 起到 %d 前用完）' % (OWN_GROUP, TIER_GROUP))
                    else:
                        g = KEYSTONE_GROUP if c.get('keystone') else TIER_GROUP
                        while g in groups and not c.get('keystone'):
                            g += 1
                    groups.add(g)
                    tid = self.pin('EquipParamAccessory', 'tal/%s/%s' % (c['key'], r_), self.free_acc)
                    for fld, val in (('refId', f['sp'][r_]), ('iconId', c['icon']), ('accessoryGroup', g)):
                        setf(self.AC, t, fld, val)
                    self.rows['EquipParamAccessory'][tid] = bytes(t)
                    f['tal'][r_] = tid
            rules = self.resolve_logic(c, made)
            self.rule_extras(c, list(f['sp'].values()), made, rules)
            f['runtime'] = {'rules': rules, 'vals': [list(x) if isinstance(x, tuple) else x for x in c['vals']]}
            if c.get('iframes'):                        # 身上有就无敌的效果（dark_core 设翻滚同款的无敌位）
                f['runtime']['iframes'] = sorted({i for name in c['iframes'] for i in made[name].values()})
            if c.get('hks'):                            # 导出给玩家脚本的效果（DR_AFFIX[名字] = 各稀有度的编号，DR_AnyOn 查）
                one = lambda x: x if isinstance(x, list) else [x]   # 叠层的行（每层一行）全都导出
                f['runtime']['hks'] = {k: [i for r in RAR for i in one(made[name].get(r, -1))] for k, name in c['hks'].items()}
            gear[c['key']] = f
            self.report.append('积木词条 %s（%s%s%s）：%s；规则 %s' % (
                c['suffix'][1], '、'.join(c['slots']), '，仅' + '/'.join(rars) if len(rars) < len(RAR) else '', ('，规则改变器' + ('（单独护符组）' if c.get('own_group') else '')) if c.get('keystone') else '',
                ' / '.join(f['text'][r]['zh'] for r in RAR if r in f['text']), [r['type'] for r in rules]))

    def custom_shared(self, weapon):
        """新词条共用的行（见 CRUSH_TIERS / BLOODRAGE 上面的说明）；血狂加进武器常驻效果池 special。返回 affixes.json 的 runtime"""
        bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'SpEffect.xml')))[0] if nb}

        def sp(src, pin=None, **vals):         # pin：装备记录里会存的（武器特效的标记），按名字锁定编号
            r = self.row_of('SpEffectParam', src)
            for fld, v in vals.items():
                setf(self.SP, r, fld, v)
            new = lambda: self.new_id('SpEffectParam', TIER_SP)
            i = self.pin('SpEffectParam', pin, new) if pin else new()
            self.rows['SpEffectParam'][i] = bytes(r)
            return i
        neutral = dict(iconId=-1, fallDamageRate=1.0, spCategory=0)
        # charging：寸劲（原沉稳，2026-11-05 改成 DLL 跑的 quickcharge，不再有积木规则用它）蓄力中加速要的脉冲，同 reload 固定建（放最后，前面几个的编号不动）
        marks = {k: sp(360400, effectEndurance=e, **neutral)
                 for k, e in (('roll', 0.2), ('reload', 0.2), ('attack', 0.1), ('hit', 0.1), ('charging', 0.2))}
        # 积木词条用到、上面还没有的事件（握法 grip1 / grip2 持续状态、style 切换……）：玩家脚本每次发都续 0.1 秒
        for e in sorted(getattr(self, 'rule_events', set()) - set(marks)):
            if e in self.event_rows:                    # 词条自己建的状态行（比如周围出血后 30 秒）
                marks[e] = self.event_rows[e]
            elif e == 'land':                           # 命中：挂给被打的一方（dark_core 扫周围角色），听它的词条标记带命中时触发
                marks[e] = self.enemy_sp(effectEndurance=0.1)   # （要 stateInfo 152 才起作用，见 ATK_OCCUR_STATE）
                for m in sorted(self.land_markers):
                    row = bytearray(self.rows['SpEffectParam'][m])
                    if getf(self.SP, row, 'stateInfo') not in (0, ATK_OCCUR_STATE):
                        raise RuntimeError('听命中的词条标记 %d 已经有状态 %d，挂不上命中时触发' % (m, getf(self.SP, row, 'stateInfo')))
                    setf(self.SP, row, 'stateInfo', ATK_OCCUR_STATE)
                    setf(self.SP, row, 'atkOccurrenceSpEffectId', marks[e])
                    self.rows['SpEffectParam'][m] = bytes(row)
                # LAND_GEN（用户 2026-11-05 测试者报告「任何带巨人杀手的武器附魔连面板都没有」）：武器的常驻效果里有命中状态 152
                # （原版油脂、附魔祷告就是这个状态），游戏就不给这把武器附魔。武器特效的标记不再带它，改成戴着时 dark_core 给人挂这一行
                # （152 + 命中时给对方挂 land，同防具上听命中的词条标记；挂在人身上的不影响附魔）
                if self.weapon_land_rules:
                    gen = sp(360400, effectEndurance=-1.0, stateInfo=ATK_OCCUR_STATE, atkOccurrenceSpEffectId=marks[e], **neutral)
                    for rules in self.weapon_land_rules:
                        rules.append(dict(type='while', effect=[gen] * len(RAR)))
                    self.report.append('命中检测：%d 种听命中的武器特效改成戴着时给人挂 %d（武器的常驻效果不带命中状态，附魔才上得去）' % (
                        len(self.weapon_land_rules), gen))
                # 连击值（用户 2026-11-01：拉塔恩不在距离列表里，见 HIT_ACC）：玩家自己身上的命中累积，每打中一下给自己挂 hit_self；
                # dark_core 戴着听命中的词条时一直挂着 hit_acc，hit_self 一来就扫全场找身上刚出现命中标记的角色。
                # 找不到不再按距离算打中了最近的敌人（同日：词条自己的攻击、打中物件也会涨连击值，翻滚贴着的敌人被巨人杀手扣血）
                marks['hit_self'] = sp(360400, effectEndurance=0.1, **neutral)
                fire = sp(HIT_ACC[1], cycleOccurrenceSpEffectId=marks['hit_self'])
                marks['hit_acc'] = sp(HIT_ACC[0], accumuOverFireId=fire)
            elif e in FOE_HIT_MARKS:                    # 箭 / 投掷道具打中：子弹命中时挂给被打的一方（重随 rr/affixes.py 的 ranged_hits 塞进子弹的效果位），
                marks[e] = self.enemy_sp(effectEndurance=0.1)   # dark_core 扫敌人看谁身上刚出现（不是挂给射手：那个字段是发射时挂的）
            else:
                marks[e] = sp(360400, effectEndurance=0.1, **neutral)
        # 碾压的冲击波：每档 攻击（只有物理 + 削韧）→ 子弹（半径 2.5、去掉特效）→ 行为 → 放行为的瞬间效果（照 505）
        bu0 = self.row_of('Bullet', CRUSH_BULLET)
        be0 = self.row_of('BehaviorParam_PC', CRUSH_BEH)
        at0 = self.row_of('AtkParam_Pc', getf(self.BU, bu0, 'atkId_Bullet'))
        AT_SP = [n for n in self.AT if n.startswith('spEffectId')]

        def shock(dmg):
            """一套物理冲击波（攻击 → 子弹 → 行为 → 放行为的瞬间效果），返回（效果, 攻击行）"""
            at = bytearray(at0)
            for fld, _z, _e in ATK_ELEM:
                setf(self.AT, at, fld, 0)
            for fld in AT_SP:
                setf(self.AT, at, fld, -1)
            setf(self.AT, at, 'atkPhys', dmg)
            setf(self.AT, at, 'atkSuperArmor', 15.0 + dmg / 15.0)
            ai = self.new_id('AtkParam_Pc', TIER_ATK)
            self.rows['AtkParam_Pc'][ai] = bytes(at)
            bu = bytearray(bu0)
            for fld, v in (('atkId_Bullet', ai), ('hitRadius', 2.5), *SHOCK_BULLET_FIELDS.items()):
                setf(self.BU, bu, fld, v)
            bi = self.new_id('Bullet', TIER_BULLET)
            self.rows['Bullet'][bi] = bytes(bu)
            be = bytearray(be0)
            setf(self.BE, be, 'refId', bi)
            bh = self.new_id('BehaviorParam_PC', TIER_BEH)
            self.rows['BehaviorParam_PC'][bh] = bytes(be)
            s = sp(505, effectEndurance=0.0, motionInterval=0.0, behaviorId=bh, invocationConditionsStateChange1=0,
                   invocationConditionsStateChange2=0, dmypolyId=GROUND_DMY)
            row = bytearray(self.rows['SpEffectParam'][s])
            o, bp = bits['isWaitModeDelete']
            row[o] &= ~(1 << bp)
            self.rows['SpEffectParam'][s] = bytes(row)
            return s, ai
        tiers = [[dmg, shock(dmg)[0]] for dmg in CRUSH_TIERS]
        crush_live = [list(shock(CRUSH_TIERS[0])) for _ in range(CRUSH_LIVE_N)]   # [效果, 攻击行]：碾压的实时伤害
        shock_live = [list(shock(CRUSH_TIERS[0])) for _ in range(SHOCK_LIVE_N)]   # 同上：积木引擎的物理冲击波（反震 / 坠星）
        # 法球的魔力冲击波：同上，伤害放在魔力，保留魔力爆炸的特效，半径 3.5
        magic = []
        for dmg in CRUSH_TIERS:
            at = bytearray(at0)
            for fld, _z, _e in ATK_ELEM:
                setf(self.AT, at, fld, 0)
            for fld in AT_SP:
                setf(self.AT, at, fld, -1)
            setf(self.AT, at, 'atkMag', dmg)
            setf(self.AT, at, 'atkSuperArmor', 10.0 + dmg / 30.0)
            ai = self.new_id('AtkParam_Pc', TIER_ATK)
            self.rows['AtkParam_Pc'][ai] = bytes(at)
            bu = bytearray(bu0)
            for fld, v in (('atkId_Bullet', ai), ('hitRadius', 3.5)):
                setf(self.BU, bu, fld, v)
            bi = self.new_id('Bullet', TIER_BULLET)
            self.rows['Bullet'][bi] = bytes(bu)
            be = bytearray(be0)
            setf(self.BE, be, 'refId', bi)
            bh = self.new_id('BehaviorParam_PC', TIER_BEH)
            self.rows['BehaviorParam_PC'][bh] = bytes(be)
            s = sp(505, effectEndurance=0.0, motionInterval=0.0, behaviorId=bh, invocationConditionsStateChange1=0,
                   invocationConditionsStateChange2=0, dmypolyId=GROUND_DMY)
            row = bytearray(self.rows['SpEffectParam'][s])
            o, bp = bits['isWaitModeDelete']
            row[o] &= ~(1 << bp)
            self.rows['SpEffectParam'][s] = bytes(row)
            magic.append([dmg, s])
        # 法球射出去的辉石魔砾（迸发 / 星群）：子弹照原版 10400000（飞行 / 命中特效、自动追踪都是原版的），追踪加强、
        # 没锁定时也在射程内找目标（nolockTargetDist -1 = 按射程）；攻击照它的 40000 只留魔力、固定数值（不按法杖补正，说明写的是固定伤害）。
        # 行为 / 放行为的瞬间效果同上（玩家自己挂上就射出一颗）
        pebble = []
        pb0 = self.row_of('Bullet', PEBBLE_BULLET)
        pa0 = self.row_of('AtkParam_Pc', getf(self.BU, pb0, 'atkId_Bullet'))
        sb0 = self.row_of('Bullet', STAR_BULLET)
        sb1 = self.row_of('Bullet', STAR_BULLET1)
        at_bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'AtkParam.xml')))[0] if nb}
        bu_bits = {n: (o, bp) for n, fc, o, bp, nb, _ in pdm.layout(pdm.load(os.path.join(RR, 'data', 'BulletParam.xml')))[0] if nb}
        for dmg in PEBBLE_TIERS:
            at = bytearray(pa0)
            for fld, _z, _e in ATK_ELEM:
                setf(self.AT, at, fld, 0)
            for fld in AT_SP:
                setf(self.AT, at, fld, -1)
            setf(self.AT, at, 'atkMag', dmg)
            setf(self.AT, at, 'atkSuperArmor', ORB_POISE)
            o, bp = at_bits['isAddBaseAtk']
            at[o] &= ~(1 << bp)
            ai = self.new_id('AtkParam_Pc', TIER_ATK)
            self.rows['AtkParam_Pc'][ai] = bytes(at)
            bu = bytearray(sb0)
            # 锁定时的射击角度限制、射程加成照魔砾（星这两项是 0）
            for fld, v in (('atkId_Bullet', ai), ('homingAngle', STAR_HOMING), ('nolockTargetDist', -1.0),
                           ('lockShootLimitAng', getf(self.BU, pb0, 'lockShootLimitAng')), ('spBulletDistUpRate', getf(self.BU, pb0, 'spBulletDistUpRate'))):
                setf(self.BU, bu, fld, v)
            o, bp = bu_bits['isEnableAutoHoming']
            bu[o] |= 1 << bp
            bi2 = self.new_id('Bullet', TIER_BULLET)
            self.rows['Bullet'][bi2] = bytes(bu)
            # 第一段（先盘旋）：时间到放出上面那颗（第二段）；没锁定也自动找目标、射程加成同第二段
            b1 = bytearray(sb1)
            for fld, v in (('atkId_Bullet', ai), ('HitBulletID', bi2), ('nolockTargetDist', -1.0),
                           ('lockShootLimitAng', getf(self.BU, pb0, 'lockShootLimitAng')), ('spBulletDistUpRate', getf(self.BU, pb0, 'spBulletDistUpRate'))):
                setf(self.BU, b1, fld, v)
            b1[o] |= 1 << bp
            bi = self.new_id('Bullet', TIER_BULLET)
            self.rows['Bullet'][bi] = bytes(b1)
            be = bytearray(be0)
            setf(self.BE, be, 'refId', bi)
            bh = self.new_id('BehaviorParam_PC', TIER_BEH)
            self.rows['BehaviorParam_PC'][bh] = bytes(be)
            s = sp(505, effectEndurance=0.0, motionInterval=0.0, behaviorId=bh, invocationConditionsStateChange1=0,
                   invocationConditionsStateChange2=0)
            row = bytearray(self.rows['SpEffectParam'][s])
            o, bp = bits['isWaitModeDelete']
            row[o] &= ~(1 << bp)
            self.rows['SpEffectParam'][s] = bytes(row)
            pebble.append([dmg, s, bi])          # 带子弹编号：dark_core 可以直接从法器点位射出（orbshot.rs）
        # 挂给敌人的扣血（目标层）：照出血发作 6401 的节奏（1 秒、2 秒一跳 = 挂上就扣一次）
        hpdmg = [[dmg, self.enemy_sp(direct=True, effectEndurance=1.0, motionInterval=2.0, changeHpPoint=dmg)] for dmg in HP_TIERS]
        # 精确扣血（用户 2026-11-09）：dark_core 挂之前把这次的伤害写进行里，一行不能同时给两个值，留一批轮流用；范围效果可能同一帧打中很多敌人。
        # 编号固定（pin），不占上面的自动编号，别的行的号不变
        hppool = [self.enemy_sp(direct=True, pin=HP_POOL0 + k, effectEndurance=1.0, motionInterval=2.0, changeHpPoint=HP_TIERS[0]) for k in range(HP_POOL_N)]
        # 异常累积
        buildups = {}
        for kind, src, fld in BUILDUP_SRC:
            extra = {}
            if kind == 'frost' and getf(self.SP, self.row_of('SpEffectParam', src), 'cycleOccurrenceSpEffectId') in (-1, 0):
                extra['cycleOccurrenceSpEffectId'] = self.rime_emit   # 冻伤发作行重随时会接霜铠的附近判定（rimeguard 的 hooks），复制的行先接上
            buildups[kind] = [[n, sp(src, **{fld: n}, **extra)] for n in BUILDUP_AMOUNTS]
            for _, i in buildups[kind]:                 # dark_core 直接挂给敌人：有的原版行没有「挂给自己」位，挂不上
                self.allow_direct(i)
        # 血狂
        b = BLOODRAGE
        marker = sp(360400, pin='special/bloodrage', **neutral)
        stacks = [sp(360400, iconId=status_icon('bloodrage', n), vfxId=VFX_BLOODRAGE, fallDamageRate=1.0, spCategory=0,
                     effectEndurance=b['reset'] + 1.0, **{x: 1.0 + n * b['pct'] / 100.0 for x in ATK_RATES}) for n in range(1, b['max'] + 1)]
        # 狼魂的日志标记（玩家脚本挂，dll\dark_core 看到写日志）：攻击类状态里调到防御取消 / 原版取消时机防御了 / 尝试防御 / 防御成功；
        # 加在这里最后，前面的编号不变
        marks.update({k: sp(360400, effectEndurance=e, **neutral)
                      for k, e in (('wolf_atk', 0.1), ('wolf_van', 0.2), ('wolf_try', 0.2), ('wolf_ok', 0.2))})
        # 霸王的日志标记：战技霸体期间受击（玩家脚本按无硬直处理了）；同样只能接在最后
        marks['overlord_hit'] = sp(360400, effectEndurance=0.1, **neutral)
        # 血狂听的新的一下轻击（sc/hks.py NEW_EVENTS 的 light）；同样接在最后
        marks['light_new'] = sp(360400, effectEndurance=0.1, **neutral)
        # 血瓮（hp_flat）：血量上限 +1％ ~ +HPMAX_TIERS％ 的档位，dark_core 按基础血量上限挑一档挂着；同样接在最后
        hpmax = [[k, sp(360400, effectEndurance=-1.0, maxHpRate=1.0 + k / 100.0, **neutral)] for k in range(1, HPMAX_TIERS + 1)]
        # 武器的特殊效果词条位：pool 里每种 {键, 效果, 权重, 能出的武器类型（空 = 全部）, 短名, 说明}
        weapon['special'] = {'count': WEAPON_SPECIAL_COUNT,
                             'pool': [{'key': 'bloodrage', 'sp': marker, 'weight': 1, 'types': [], 'name': list(b['name']),
                                       'zh': b['zh'], 'en': b['en']}]
                                     + [{k: v for k, v in x.items() if k != 'rules'} for x in self.rule_specials]}
        self.report.append('新词条共用：脉冲标记 %s；碾压冲击波 %d 档（%d ~ %d）；血狂 标记 %d、%d 层（武器特殊效果词条位，传说 %s%% 有特效）' % (
            marks, len(tiers), CRUSH_TIERS[0], CRUSH_TIERS[-1], marker, len(stacks), WEAPON_SPECIAL_COUNT['Legendary'].get('1', 0)))
        return {'marks': marks, 'crush': tiers, 'crush_live': crush_live, 'shock_live': shock_live, 'bursts': {'magic': magic, 'hp': hpdmg, 'pebble': pebble, 'hpmax': hpmax}, 'hp_pool': hppool, 'buildups': buildups,
                'bloodrage': {'marker': marker, 'stacks': stacks, 'hp': b['hp'], 'reset': b['reset']},
                # 积木武器特效的规则：标记各稀有度同一个（武器常驻效果位）
                'special_rules': {x['key']: {'markers': [x['sp']] * len(RAR), 'rules': x['rules']} for x in self.rule_specials}}

    def merge_and_balance(self, gear):
        """词条合并（变体组、圣杯瓶）和权重（每条一样、变体组算一条），见 ARCHETYPES / VARIANTS / FLASK 上面的说明。
        要在 custom_shared 后面：圣杯瓶的新行接在最后，前面的编号不变"""
        def find(kw):
            hit = [k for k, f in gear.items() if kw in f['zh']]
            if len(hit) != 1:
                raise RuntimeError('词条合并：「%s」对上了 %d 条词条' % (kw, len(hit)))
            return hit[0]
        # 圣杯瓶：照蓝瓶的效果行，红瓶倍率同蓝瓶
        blue = gear[find(FLASK['src'])]
        groups = {getf(self.AC, row, 'accessoryGroup') for row in self.rows['EquipParamAccessory'].values()}
        fl = dict(zh=FLASK['zh'].replace('%s', '#'), en=FLASK['en'].replace('%s', '#').replace('%%', '%'), suffix=list(FLASK['suffix']),
                  group=blue['group'], sp={}, vals={}, text={}, tal={}, weight=dict(blue['weight']))
        for r in RAR:
            if r not in blue['sp']:
                continue
            row = bytearray(self.row_of('SpEffectParam', blue['sp'][r]))
            rate = getf(self.SP, row, 'changeMpEstusFlaskCorrectRate')
            setf(self.SP, row, 'changeHpEstusFlaskCorrectRate', rate)
            i = self.pin('SpEffectParam', 'sp/%s/%s' % (FLASK['key'], r), lambda: self.new_id('SpEffectParam', TIER_SP))
            self.rows['SpEffectParam'][i] = bytes(row)
            fl['sp'][r] = i
            v = ('%.1f' % ((rate - 1.0) * 100)).rstrip('0').rstrip('.')
            fl['text'][r] = {'zh': FLASK['zh'] % v, 'en': FLASK['en'] % v}
            fl['vals'][r] = [v]
            if r in blue['tal']:
                t = bytearray(self.rows['EquipParamAccessory'][blue['tal'][r]])
                g = TIER_GROUP
                while g in groups:
                    g += 1
                groups.add(g)
                tid = self.pin('EquipParamAccessory', 'tal/%s/%s' % (FLASK['key'], r), self.free_acc)
                for fld, val in (('refId', i), ('accessoryGroup', g)):
                    setf(self.AC, t, fld, val)
                self.rows['EquipParamAccessory'][tid] = bytes(t)
                fl['tal'][r] = tid
        for kw in FLASK['retire']:
            gear[find(kw)]['retired'] = True
        for k in RETIRED_KEYS:                          # 停用的新词条：不再掉落，已有的装备照旧
            if k in gear:
                gear[k]['retired'] = True
        gear[FLASK['key']] = fl
        # 流派（只做报告里的分类）、变体组
        for k, f in gear.items():
            f['archetype'] = next((a for a, kws in ARCHETYPES if any(x in f['zh'] for x in kws)), '其他')
        only = {}
        for gk, name, members in VARIANTS:
            for kw, rar in members:
                k = find(kw)
                gear[k]['merge_group'] = [gk, name]
                if rar:
                    only[k] = set(rar)
        # 权重：池子（防具 / 护符）× 稀有度
        src = {k: dict(f['weight']) for k, f in gear.items()}
        for f in gear.values():
            f['weight_src'] = dict(f['weight'])
            f['weight_r'] = {'armor': [0] * len(RAR), 'talisman': [0] * len(RAR)}
        for pool in ('armor', 'talisman'):
            for ri, r in enumerate(RAR):
                units = defaultdict(list)                       # 变体组（或单条）→ 能出的组员
                for k, f in gear.items():
                    if f.get('retired') or src[k][pool] <= 0 or r not in f['sp'] or (k in only and r not in only[k]):
                        continue
                    if pool == 'talisman' and r not in f['tal']:
                        continue
                    units[(f.get('merge_group') or [k])[0]].append(k)
                for members in units.values():
                    for k in members:
                        gear[k]['weight_r'][pool][ri] = max(1, UNIFORM_W // len(members))
        for f in gear.values():
            f['weight'] = {p: f['weight_r'][p][RAR.index('Legendary')] for p in ('armor', 'talisman')}
        # 报告：传说档各流派的掉率（调整前后）
        for pool, zh in (('armor', '防具'), ('talisman', '护符')):
            def by(wget):
                acc = defaultdict(float)
                for f in gear.values():
                    acc[f['archetype']] += wget(f)
                t = sum(acc.values()) or 1.0
                return {a: 100 * v / t for a, v in acc.items()}
            before = by(lambda f: 0 if f is fl else f['weight_src'][pool] if f['sp'].get('Legendary') else 0)
            after = by(lambda f: f['weight_r'][pool][RAR.index('Legendary')])
            self.report.append('流派掉率（%s，传说，调整前 → 后）：%s' % (zh, '；'.join(
                '%s %.1f%%→%.1f%%' % (a, before.get(a, 0), after.get(a, 0)) for a, _ in ARCHETYPES if before.get(a) or after.get(a))))
        n_slot = len({(f.get('merge_group') or [k])[0] for k, f in gear.items() if not f.get('retired')})
        self.report.append('词条合并：%d 种词条族（含停用 %d 种）→ 抽词条时 %d 个位置；变体组 %d 个；新增圣杯瓶 %s' % (
            len(gear), sum(1 for f in gear.values() if f.get('retired')), n_slot, len(VARIANTS), fl['sp']))

    def sort_templates(self, gear):
        """护符模板的 sortId 按词条顺序、同一词条里按稀有度：龙飨界面（暗黑商店的护符商店）只按 sortId 排，
        背包里也一样，同一种词条的几件挨在一起（原包是按稀有度分段的）"""
        for k, f in enumerate(gear.values()):
            for r in RAR:
                if r in f['tal']:
                    row = bytearray(self.rows['EquipParamAccessory'][f['tal'][r]])
                    setf(self.AC, row, 'sortId', TAL_SORT0 + k * 10 + RAR.index(r))
                    self.rows['EquipParamAccessory'][f['tal'][r]] = bytes(row)
        self.report.append('护符模板 sortId：按词条排（%d ~ %d）' % (TAL_SORT0, TAL_SORT0 + 10 * len(gear) - 1))

    # ------------------------------------------------------------ 词条数值随机（见 VAL_OVERLAP）
    @staticmethod
    def _vr_fmt(v):
        return ('%d' % round(v)) if abs(v - round(v)) < 1e-6 else ('%.1f' % v)

    @staticmethod
    def _vr_span(seq, prec):
        """各档的范围（相邻两档的中点再往外扩，相邻重叠 VAL_OVERLAP；正数不到 0）"""
        mid = [(seq[i] + seq[i + 1]) / 2 for i in range(len(seq) - 1)]
        out = []
        for i, c in enumerate(seq):
            a = mid[i - 1] if i > 0 else c - (mid[0] - c)
            b = mid[i] if i < len(mid) else c + (c - mid[-1])
            a, b = min(a, b), max(a, b)
            d = VAL_OVERLAP / (2 * (1 - VAL_OVERLAP)) * (b - a)
            a, b = a - d, b + d
            if min(seq) > 0:
                a = max(a, prec)
            out.append((round(a, 4), round(b, 4)))
        return out

    @staticmethod
    def _vr_solve_mu(c, lo, hi):
        """截断正态 [lo, hi]、σ = 中心到较远一端的一半：均值正好是 c 的 μ"""
        def tn_mean(mu, sig):
            pdf = lambda x: math.exp(-x * x / 2) / math.sqrt(2 * math.pi)
            cdf = lambda x: 0.5 * (1 + math.erf(x / math.sqrt(2)))
            al, be = (lo - mu) / sig, (hi - mu) / sig
            z = cdf(be) - cdf(al)
            return mu + sig * (pdf(al) - pdf(be)) / z if z > 1e-12 else (lo + hi) / 2
        sig = max(c - lo, hi - c, 1e-6) / 2
        a, b = lo - 6 * sig, hi + 6 * sig
        for _ in range(100):
            m = (a + b) / 2
            if tn_mean(m, sig) < c:
                a = m
            else:
                b = m
        return round((a + b) / 2, 5), round(sig, 5)

    def _vr_texts(self, f, rs, idx, seqs, prec, main, cap=None):
        """各档的范围、抽样参数、说明模板（变化的数换成 {j}）和词条百科的范围说明：{r: {lo, hi, c, mu, sigma, tpl, span}}；
        说明里找不到要换的数时返回 (None, '中文' / '英文')。
        中文按词条说明模板（f['zh'] 里的 #，第 j 个 # = vals[j]）找到数字的位置，英文按数找（顺序可能和中文不同、原包有的数写错了）；正负号照原文"""
        fmt = self._vr_fmt

        def put(g, v):
            return (g[0] if g[0] in '+-' else '') + (v if isinstance(v, str) else fmt(abs(v)))

        def by_tpl(t, tpl, new, n):
            parts = tpl.split('#')
            if len(parts) < 2 or len(parts) - 1 != n:
                return None                                            # # 的个数和数的个数对不上（有的数写死在说明里）：按数找
            m = re.match('(?s)' + ('(' + NUM.pattern + ')').join(re.escape(x) for x in parts) + '$', t)
            if not m or len(m.groups()) != len(parts) - 1:
                return None
            out, last = [], 0
            for j in range(len(parts) - 1):
                out.append(t[last:m.start(j + 1)])
                out.append(put(m.group(j + 1), new[j]) if j in new else m.group(j + 1))
                last = m.end(j + 1)
            return ''.join(out) + t[last:]

        # 第 j 个数在说明里是第几个数（中英文各算各的，英文的顺序可能不同）：在这个数和别的数不重的那档里找
        perms = {}
        for lang in ('zh', 'en'):
            perm = perms.setdefault(lang, {})
            for r in rs:
                ms = NUM.findall(f['text'][r][lang])
                for j in idx:
                    pos = [k for k, x in enumerate(ms) if abs(float(x)) == abs(float(f['vals'][r][j]))]
                    if len(pos) == 1:
                        perm.setdefault(j, pos[0])

        def by_pos(t, old, new, perm):
            ms = list(NUM.finditer(t))
            pick = {}
            for j in new:
                hits = [x for x in ms if abs(float(x.group())) == abs(float(old[j]))]
                if j in perm and perm[j] < len(ms) and abs(float(ms[perm[j]].group())) == abs(float(old[j])):
                    pick[j] = ms[perm[j]]
                elif len(ms) == len(old) and abs(float(ms[j].group())) == abs(float(old[j])):
                    pick[j] = ms[j]
                elif len(hits) == 1:
                    pick[j] = hits[0]
                elif len(ms) == 1 and len(old) == 1:
                    pick[j] = ms[0]                                    # 只有一个数、原包英文写的数不对：照中文的换
                else:
                    return None
            for j, x in sorted(pick.items(), key=lambda kv: -kv[1].start()):
                t = t[:x.start()] + put(x.group(), new[j]) + t[x.end():]
            return t

        def interp(i, v):
            """跟着主数变的数：按四档的点（主数, 这个数）分段插值，两头按端上那段外推"""
            pts = sorted(zip(seqs[main], seqs[i]))
            k = 0 if v <= pts[0][0] else (len(pts) - 2 if v >= pts[-1][0] else next(k for k in range(len(pts) - 1) if pts[k][0] <= v <= pts[k + 1][0]))
            (x0, y0), (x1, y1) = pts[k], pts[k + 1]
            y = y0 + (y1 - y0) * (v - x0) / (x1 - x0) if x1 != x0 else y0
            return round(round(y / prec[i]) * prec[i], 6)

        q = lambda v, i: round(round(v / prec[i]) * prec[i], 6)
        spans = self._vr_span(seqs[main], prec[main])
        per = {}
        for ri, r in enumerate(rs):
            lo, hi = spans[ri]
            c = seqs[main][ri]
            if cap is not None:                                        # VAL_CAP：上端截到上限
                hi = min(hi, cap)
                lo = min(lo, hi)
            mu, sig = self._vr_solve_mu(c, lo, hi) if c < hi else (c, round(max(c - lo, 1e-6) / 2, 5))
            tok = {i: '{%d}' % i for i in idx}
            lo_d, hi_d = q(lo, main), q(hi, main)
            rng = {i: '%s~%s' % (fmt(abs(interp(i, lo_d) if i != main else lo_d)), fmt(abs(interp(i, hi_d) if i != main else hi_d))) for i in idx}
            txt = {}
            for nm, new in (('tpl', tok), ('span', rng)):
                zh = by_tpl(f['text'][r]['zh'], f['zh'], new, len(f['vals'][r])) or by_pos(f['text'][r]['zh'], f['vals'][r], new, perms['zh'])
                en = by_pos(f['text'][r]['en'], f['vals'][r], new, perms['en'])
                if nm == 'tpl':                                        # 每个随机的数在模板里正好一个占位
                    zh = zh if zh is not None and all(zh.count(tok[i]) == 1 for i in idx) else None
                    en = en if en is not None and all(en.count(tok[i]) == 1 for i in idx) else None
                if zh is None or en is None:
                    return None, '中文' if zh is None else '英文'
                txt[nm] = [zh, en]
            per[r] = {'lo': lo, 'hi': hi, 'c': c, 'mu': mu, 'sigma': sig, 'tpl': txt['tpl'], 'span': txt['span']}
        return per, ''

    def _vr_nums(self, f, small=True):
        """(有的稀有度, 说明里随稀有度变化的数的位置, 位置 → 各档的数)；没有能随机的数返回 None。
        small：数太小（相邻只差 1，像记忆空格 +1）的不做（一期；二期的计数本来就不在能随机的字段里，秒数、百分比照做）"""
        rs = [r for r in RAR if r in f['sp']]
        if len(rs) < 2:
            return None
        try:
            nums = [[float(x) for x in f['vals'][r]] for r in rs]
        except (KeyError, ValueError):
            return None
        if not nums[0] or any(len(x) != len(nums[0]) for x in nums):
            return None
        idx = [i for i in range(len(nums[0])) if len({x[i] for x in nums}) > 1]
        if not idx:
            return None
        seqs = {i: [x[i] for x in nums] for i in idx}
        s0 = seqs[idx[0]]
        if small and len(idx) == 1 and max(abs(x) for x in s0) <= 8 and s0[-1] - s0[0] <= 3 and all(abs(s0[j + 1] - s0[j]) <= 1 for j in range(len(s0) - 1)):
            return None                                                # 数太小（相邻只差 1）
        return rs, idx, seqs

    def value_ranges(self, gear):
        def walk(root, more=()):
            """效果链按路径展开（只走效果行之间的引用）：{路径: 编号}。more：另外几个链头（词条的 sp_more，路径 +1、+2……）"""
            out, todo = {}, [('', root)] + [('+%d' % (j + 1), x) for j, x in enumerate(more)]
            while todo:
                path, i = todo.pop(0)
                if i <= 0 or i in out.values():
                    continue
                row = self.row_of('SpEffectParam', i)
                if row is None:
                    continue
                out[path] = i
                todo += [(path + '/' + x, getf(self.SP, row, x)) for x in STEP_SPR]
            return out

        def form(xs, ys):
            """ys 是不是 xs 的干净线性关系（STEP_FORMS 里第一个对得上的）：返回 (a, b) 或 None"""
            return next(((a, b) for a, b in STEP_FORMS if all(abs(a + b * x - y) <= 1e-4 + 1e-4 * abs(y) for x, y in zip(xs, ys))), None)

        fields = [n for n in self.SP if n not in STEP_SPR and n != 'behaviorId']
        n_fam, skipped = 0, []
        for key, f in gear.items():
            name = (f.get('suffix') or ['', key])[1]
            f.pop('vrand', None)
            if f.get('retired') or ('runtime' in f and key not in STEP_RUNTIME_OK) or key in STEP_SKIP:
                continue
            got = self._vr_nums(f)
            if not got:
                continue
            rs, idx, seqs = got
            # 链：只连着效果行（没有行为 / 子弹 / 攻击），路径对齐
            ws = [walk(f['sp'][r], (f.get('sp_more') or {}).get(r, ())) for r in rs]
            if any(set(w) != set(ws[0]) for w in ws) or any(getf(self.SP, self.row_of('SpEffectParam', i), 'behaviorId') > 0
                                                            for w in ws for i in w.values()):
                continue
            own = [pth for pth in ws[0] if len({w[pth] for w in ws}) > 1]
            if '' not in own:
                continue
            own.remove('')
            own.insert(0, '')                                          # 第 0 行是链头
            # 每个字段：和哪个变化的数成干净线性关系
            fits = {}
            for pth in own:
                rows_ = [self.row_of('SpEffectParam', w[pth]) for w in ws]
                for fld in fields:
                    ys = [getf(self.SP, rw, fld) for rw in rows_]
                    if len(set(ys)) == 1:
                        continue
                    # 先认最直接的关系（±N 先于 ±0.1N），同一种关系里数大的优先
                    hit = next(((i, ab) for ab in STEP_FORMS for i in sorted(idx, key=lambda i: -max(abs(x) for x in seqs[i]))
                                if form(seqs[i], ys) == ab), None)
                    if hit:
                        fits[(pth, fld)] = hit
            if not all(any(v[0] == i for v in fits.values()) for i in idx):
                skipped.append('%s（有的数找不到对应的字段）' % name)
                continue
            # 显示精度：对应的字段都是浮点的到 0.1；有整数字段的取能让 b × 精度是整数的最小一个
            prec = {}
            for i in idx:
                ints = [abs(ab[1]) for (pp, fld), (j, ab) in fits.items() if j == i and self.SP[fld][1] not in 'fd']
                prec[i] = next(q for q in (0.1, 1.0, 10.0, 100.0) if all(abs(b * q - round(b * q)) < 1e-6 and round(b * q) != 0 for b in ints))
            main = max(idx, key=lambda i: ((max(seqs[i]) - min(seqs[i])) / prec[i], -idx.index(i)))
            per, bad = self._vr_texts(f, rs, idx, seqs, prec, main, VAL_CAP.get(key))
            if bad:
                skipped.append('%s（%s说明里找不到要换的数）' % (name, bad))
                continue
            for ri, r in enumerate(rs):
                chain = ws[ri]
                at = {chain[pth]: k for k, pth in enumerate(own)}
                refs, flds = [], []
                for k, pth in enumerate(own):
                    row = self.row_of('SpEffectParam', chain[pth])
                    for x in STEP_SPR:
                        t = getf(self.SP, row, x)
                        if t in at:
                            refs.append([k, self.SP[x][0], at[t]])
                    for (pp, fld), (i, (a, b)) in fits.items():
                        if pp == pth:
                            flds.append([k, self.SP[fld][0], self.SP[fld][1], a, b, i])
                per[r].update({'rows': [chain[pth] for pth in own], 'refs': refs, 'fields': flds})
            f['vrand'] = {'main': main, 'nums': {str(i): {'prec': prec[i], 'c': {r: seqs[i][ri] for ri, r in enumerate(rs)}} for i in idx},
                          'r': per}
            n_fam += 1
        self.report.append('词条数值随机：%d 条词条（dark_core 按身上装备写占位行）；没做的：%s' % (n_fam, '、'.join(skipped) or '无'))
        self.value_ranges_rules(gear)

    def value_ranges_rules(self, gear):
        """二期：积木规则 / 专用模块的词条（用户 2026-11-15「开始第二期吧」）。说明里每个随稀有度变化的数找它在哪：
        规则里每档一个数的字段（VAL_RULE_NUMS，几率、回复量、冷却……）、规则挂的效果行（VAL_RULE_SLOTS，按档 / 按层的效果编号，行里和数成线性关系的字段）、
        专用模块定义里的数（VAL_RUNS）。层数上限、法球上限这类计数照旧按档（改了效果的层数就对不上了），找不到的数也照旧按档。
        词条库 vrand 多 kind：'rule'（rnum：(第几条规则, 字段, a, b, 数)；各档 slots：(第几条规则, 字段, 第几层 / -1, 原来的效果编号, [(偏移, 类型, a, b, 数)])）
        或 'run'（run：(模块, 模块数值的位置, a, b, 数)）；dark_core 的积木引擎 / 模块按身上那件的位置取精确的数"""
        def fit(xs, ys):
            """a + b × N（a 是 0 或 1）：返回 (a, b) 或 None"""
            pts = [(x, y) for x, y in zip(xs, ys) if y is not None]
            if len(pts) < 2 or len({x for x, _ in pts}) < 2:
                return None
            n = len(pts)
            mx, my = sum(x for x, _ in pts) / n, sum(y for _, y in pts) / n
            b = sum((x - mx) * (y - my) for x, y in pts) / sum((x - mx) ** 2 for x, _ in pts)
            a = my - b * mx
            if abs(b) < 1e-9 or max(abs(a + b * x - y) for x, y in pts) > 1e-3 + 1e-4 * max(abs(y) for _, y in pts):
                return None
            a0 = 0.0 if abs(a) < 1e-3 else (1.0 if abs(a - 1) < 1e-3 else None)
            return None if a0 is None else (a0, round(b, 6))

        skipf = set(STEP_SPR) | {'behaviorId', 'iconId', 'stateInfo', 'spCategory'} | {n for n in self.SP if n.startswith('vfxId')}
        n_fam, skipped, parts = 0, [], Counter()
        for key, f in gear.items():
            rt = f.get('runtime') or {}
            name = (f.get('suffix') or ['', key])[1]
            if f.get('retired') or key in STEP_SKIP or key in STEP_RUNTIME_OK or not (rt.get('rules') or rt.get('run') in VAL_RUNS):
                continue
            got = self._vr_nums(f, small=False)
            if not got:
                continue
            rs, idx, seqs = got
            ri_of = {r: RAR.index(r) for r in rs}
            rnum, slots, runs = [], {r: [] for r in rs}, []
            carried = set()
            # 挂的效果行：说明里有几个各档都一样的数（交替：箭伤 #％、道具伤害 #％ 都是 20/30/40/50）时，哪个都对得上。
            # 不同的规则优先认还没被别的规则认走的那个（按规则顺序对说明里的顺序），不然后面那条规则的行也跟着第一个数变，
            # 说明里第二个数却照固定数写（用户 2026-11-23「把道具那半的数值说明也修一下」）
            slot_by = defaultdict(set)              # 数的序号 → 用它的规则
            rowrecs = []                            # 挂的效果行：(规则, 字段, 层, 各档编号, 各档行, [[字段名, (a, b), 数, 对得上的数]…])
            for nth, rule in enumerate(rt.get('rules') or []):
                for fld, v in rule.items():
                    if fld in VAL_RULE_NUMS and isinstance(v, list) and len(v) == len(RAR) and all(isinstance(x, (int, float)) for x in v):
                        for j in idx:
                            ab = fit(seqs[j], [float(v[ri_of[r]]) for r in rs])
                            if ab:
                                rnum.append([nth, fld, ab[0], ab[1], j])
                                carried.add(j)
                                break
                    if fld not in VAL_RULE_SLOTS or not isinstance(v, list) or len(v) != len(RAR):
                        continue
                    if all(isinstance(x, int) for x in v):
                        lays = [(-1, [v[ri_of[r]] for r in rs])]
                    elif all(isinstance(x, list) for x in v):
                        n = max(len(x) for x in v)
                        lays = [(L, [v[ri_of[r]][L] if L < len(v[ri_of[r]]) else None for r in rs]) for L in range(n)]
                    else:
                        continue
                    for L, ids in lays:
                        rows_ = [self.row_of('SpEffectParam', i) if i and i > 0 else None for i in ids]
                        if sum(1 for x in rows_ if x is not None) < 2:
                            continue
                        found = []
                        for fn in self.SP:
                            if fn in skipf:
                                continue
                            ys = [getf(self.SP, rw, fn) if rw is not None else None for rw in rows_]
                            if len({y for y in ys if y is not None}) < 2:
                                continue
                            fits = [(j, ab) for j in idx for ab in [fit(seqs[j], ys)] if ab]
                            free = [x for x in fits if slot_by[x[0]] <= {nth}]
                            if fits:
                                j, ab = (free or fits)[0]
                                found.append([fn, ab, j, [x[0] for x in fits]])
                        if not found:
                            continue
                        rowrecs.append((nth, fld, L, ids, rows_, found))
                        carried.update(x[2] for x in found)
                        for x in found:
                            slot_by[x[2]].add(nth)
            if rt.get('run') in VAL_RUNS:
                vals = rt.get('vals') or []
                for vi in VAL_RUNS[rt['run']]:
                    col = [v[vi] if isinstance(v, list) else (v if vi == 0 else None) for v in vals]
                    if len(col) != len(RAR) or any(x is None for x in col):
                        continue
                    for j in idx:
                        ab = fit(seqs[j], [float(col[ri_of[r]]) for r in rs])
                        if ab:
                            runs.append([rt['run'], vi, ab[0], ab[1], j])
                            carried.add(j)
                            break
            # 说明里有个数哪儿都没认走、但和一行里几个字段认的那个数各档一样（残心：攻击力、削韧都是 15 / 22 / 30 / 40，削韧那个字段先认了攻击力那个数；
            # 用户 2026-11-24）：这一行最后一个这样的字段改认它。数一样、位置一样，挂上去的数不变，说明里两个数就都按位置写，
            # 不然只有第一个数随机、第二个照固定数写。只拿谁都没认的数，不动已经分给别的规则的（交替）
            for k in idx:
                if k in carried:
                    continue
                for *_x, found in rowrecs:
                    cnt = Counter(x[2] for x in found)
                    cand = [x for x in found if x[2] != k and k in x[3] and seqs[x[2]] == seqs[k] and cnt[x[2]] >= 2]
                    if cand:                        # 原来认的那个数这一行还剩别的字段
                        cand[-1][2] = k
                        carried.add(k)
            for nth, fld, L, ids, rows_, found in rowrecs:
                for ri, r in enumerate(rs):
                    if ids[ri] and ids[ri] > 0 and rows_[ri] is not None:
                        slots[r].append([nth, fld, L, ids[ri], [[self.SP[fn][0], self.SP[fn][1], ab[0], ab[1], j] for fn, ab, j, _f in found]])
            ridx = [j for j in idx if j in carried]
            if not ridx:
                skipped.append('%s（数不在规则字段、挂的效果行或模块数值里）' % name)
                continue
            # 显示精度：整数字段（规则的精力 / 专注点数、效果行的整数字段）取整，其余 0.1
            int_rule = {'stamina', 'fp', 'hp'}
            prec = {}
            for j in ridx:
                ints = [abs(b) for nth, fld, a, b, jj in rnum if jj == j and fld in int_rule]
                ints += [abs(fb) for r in rs for x in slots[r] for off, fc, fa, fb, jj in x[4] if jj == j and fc not in 'fd']
                prec[j] = next(q for q in (0.1, 1.0, 10.0, 100.0) if all(abs(b * q - round(b * q)) < 1e-6 and round(b * q) != 0 for b in ints))
            main = max(ridx, key=lambda i: ((max(seqs[i]) - min(seqs[i])) / prec[i], -ridx.index(i)))
            per, bad = self._vr_texts(f, rs, ridx, {j: seqs[j] for j in ridx}, prec, main)
            if bad:
                skipped.append('%s（%s说明里找不到要换的数）' % (name, bad))
                continue
            for r in rs:
                per[r]['slots'] = slots[r]
            f['vrand'] = {'kind': 'run' if runs and not rnum and not any(slots.values()) else 'rule', 'main': main,
                          'nums': {str(i): {'prec': prec[i], 'c': {r: seqs[i][ri] for ri, r in enumerate(rs)}} for i in ridx},
                          'r': per, 'rnum': rnum, 'run': runs}
            parts.update(['规则里的数'] * bool(rnum) + ['挂的效果行'] * bool(any(slots.values())) + ['模块数值'] * bool(runs))
            n_fam += 1
        self.report.append('词条数值随机（二期）：%d 条词条（%s）；没做的：%s' % (n_fam, dict(parts), '、'.join(skipped) or '无'))

    # ------------------------------------------------------------ 原版里没有的行：词条效果连同它引用的行
    def closure(self, start):
        SP_REF = ('replaceSpEffectId', 'cycleOccurrenceSpEffectId', 'atkOccurrenceSpEffectId', 'accumuOverFireId', 'accumuUnderFireId',
                  'spiritDeathSpEffectId', 'applyIdOnGetSoul')
        BU_SP = [n for n in self.BU if n.startswith('spEffectId')] + ['spEffectIDForShooter']
        AT_SP = [n for n in self.AT if n.startswith('spEffectId')]
        seen = defaultdict(set)
        todo = [('SpEffectParam', s) for s in start]
        while todo:
            p, i = todo.pop()
            if i <= 0 or i in seen[p]:
                continue
            seen[p].add(i)
            row = self.Pk[p].rows.get(i) if p in self.Pk else None
            if row is None:
                continue
            if p == 'SpEffectParam':
                todo += [('SpEffectParam', getf(self.SP, row, f)) for f in SP_REF]
                b = getf(self.SP, row, 'behaviorId')
                if b > 0:
                    todo.append(('BehaviorParam_PC', b))
            elif p == 'BehaviorParam_PC':
                rt, rid = getf(self.BE, row, 'refType'), getf(self.BE, row, 'refId')
                todo.append(({0: 'AtkParam_Pc', 1: 'Bullet', 2: 'SpEffectParam'}.get(rt, '-'), rid))
            elif p == 'Bullet':
                todo.append(('AtkParam_Pc', getf(self.BU, row, 'atkId_Bullet')))
                todo += [('SpEffectParam', getf(self.BU, row, f)) for f in BU_SP]
                todo += [('Bullet', getf(self.BU, row, f)) for f in ('intervalCreateBulletId', 'HitBulletID')]
            elif p == 'AtkParam_Pc':
                todo += [('SpEffectParam', getf(self.AT, row, f)) for f in AT_SP]
        n_new, n_mod = Counter(), Counter()
        for p, ids in seen.items():
            for i in ids:
                row = self.Pk[p].rows.get(i)
                if row is None:
                    continue
                v = self.Pv[p].rows.get(i)
                if v is None:
                    self.rows[p][i] = bytes(row)
                    n_new[p] += 1
                elif bytes(v) != bytes(row):
                    n_mod[p] += 1
        self.report.append('原版里没有、要插进去的行（词条效果连同引用）：%s' % '，'.join('%s %d' % kv for kv in sorted(n_new.items())))
        if n_mod:
            self.report.append('[注意] 词条引用到原包改过的原版行（这些用原版的）：%s' % dict(n_mod))


# 叠加说明（用户 2026-11-03：「给不能叠加的都加上说明」「分为同稀有度不能叠加和取最高稀有度」；
# 2026-11-13「同稀有度不叠加改为不同稀有度可叠加 同名取最高稀有度也改得顺口一点」）。
# 原版类型的数值词条同稀有度也叠加（rr/affixes.py attr_copies 建副本），不写说明；其余两种：
#   不同稀有度可叠加：效果行是游戏原生的，不同稀有度是不同效果、能叠，同稀有度是同一个效果只算一次
#     （附近触发、会再触发别的效果的原包词条、狂怒突刺，新词条里靠原生效果的饮血）。原包的「同稀有度仅生效一次」「不叠加」统一改成这句；
#   取最高稀有度：新词条（dark_core 积木引擎 / 玩家脚本 / dark_core 只认身上稀有度最高的那件）、武器特效（双持时）。
# 命中时触发（原句「与同稀有度的相同或其他［命中时］效果不叠加」，2026-11-13 用户同意改成 STACK_ONHIT）、召唤类附近触发「同名效果不叠加」
# 规则不一样，各用各的说法
STACK_SAME = ('不同稀有度可叠加', 'Stacks with different rarities')
STACK_TOP = ('同名词条只有稀有度最高的生效', 'Only the highest rarity of this effect applies')
STACK_TOP_WPN = ('双持时，同名特效只有稀有度最高的生效', 'When dual-wielding, only the highest rarity of this special applies')
# 以前的说法（已经生成过的文字里可能还有）：换成上面的
STACK_RENAMED = {'同稀有度不叠加': STACK_SAME[0], '同名词条取最高稀有度': STACK_TOP[0], '同名特效取最高稀有度（双持时）': STACK_TOP_WPN[0]}
STACK_ONHIT = ('同稀有度的命中时效果只能生效一个', 'Only one on-hit effect of the same rarity applies')
STACK_RENAMED_EN = {'Does not stack with the same rarity': STACK_SAME[1]}
STACK_RENAMED['与同稀有度的相同或其他［命中时］效果不叠加'] = STACK_ONHIT[0]
STACK_RENAMED_EN.update({"Don't stack with same or other [on hit] effects of same rarity": STACK_ONHIT[1],
                         "Don't stack with same or others [on hit] effects of same rarity": STACK_ONHIT[1]})
STACK_OLD = {'同稀有度仅生效一次': STACK_SAME[0], '不叠加': STACK_SAME[0], **STACK_RENAMED}
STACK_OLD_EN = {'Stacks only once per rarity': STACK_SAME[1], "Doesn't stack": STACK_SAME[1], **STACK_RENAMED_EN}
STACK_NATIVE_NEW = ('dr_blood_drink',)    # 新词条里效果是游戏原生的（打倒敌人回血）：不同稀有度能叠
STACK_ALL_NEW = ('dr_gale',)              # 新词条里同名同稀有度也叠加的（rr/affixes.py STACK_NEW_KEYS 建副本，玩家脚本按件数加）：不写叠加说明
# 原版类型的数值词条里改成同名只认稀有度最高的那件（用户 2026-11-24「蓄力重击伤害和两个双手握持伤害改为只生效最高稀有度」；
# 另一个双手的「怒意」是新词条，本来就只认最高）：说明加 STACK_TOP，词条库标 stack = 'top'，rr/affixes.py 写 famtop 行、不建叠加副本，
# dark_core gear/valfx.rs 同名只挂最好的那件（稀有度最高、同稀有度数值最高）
STACK_TOP_NATIVE = ('increase_charged_attacks_by_pct', 'increase_damage_with_two_handed_weapons_by_pct')
# 会再触发别的效果、但同稀有度也能叠的（rr/affixes.py 的 STACK_CHAIN_KEYS，连再挂的效果一起建副本）：去掉原包的不叠加说明。用户 2026-11-05「翔空箭词条改为可以叠加」
STACK_FREE = ('increase_range_of_bows_by_pct_and_raise_attack_p',)


def stack_notes(gear, weapon, report):
    """给不能叠加的词条说明加最后一行（见 STACK_SAME / STACK_TOP），原包的旧说法换成统一的；要在流派分类（merge_and_balance）之后"""
    def fix(t, lang):
        old = STACK_OLD if lang == 'zh' else STACK_OLD_EN
        return '\n'.join(old.get(l, l) for l in t.split('\n'))

    n = {'same': 0, 'top': 0, 'old': 0}
    for k, f in gear.items():
        for r, t in f.get('text', {}).items():
            before = (t['zh'], t['en'])
            t['zh'], t['en'] = fix(t['zh'], 'zh'), fix(t['en'], 'en')
            n['old'] += before != (t['zh'], t['en'])
        f['zh'], f['en'] = fix(f['zh'], 'zh'), fix(f['en'], 'en')
        if k in STACK_FREE:
            drop = lambda s, line: '\n'.join(l for l in s.split('\n') if l != line)
            for t in list(f.get('text', {}).values()) + [f]:
                t['zh'], t['en'] = drop(t['zh'], STACK_SAME[0]), drop(t['en'], STACK_SAME[1])
        if (f.get('group') != '新词条' and k not in STACK_TOP_NATIVE) or f.get('retired') or k in STACK_ALL_NEW:
            continue
        note = STACK_SAME if k in STACK_NATIVE_NEW else STACK_TOP
        if k in STACK_TOP_NATIVE:
            f['stack'] = 'top'
        for r in RAR:
            if r in f.get('text', {}):
                for lang, line in zip(('zh', 'en'), note):
                    if line not in f['text'][r][lang].split('\n'):
                        f['text'][r][lang] += '\n' + line
        for lang, line in zip(('zh', 'en'), note):
            if line not in f[lang].split('\n'):
                f[lang] += '\n' + line
        n['same' if note is STACK_SAME else 'top'] += 1
    for s in weapon['special']['pool']:
        s['archetype'] = SPECIAL_ARCHETYPE.get(s['key']) or next((a for a, kws in ARCHETYPES if any(x in s['zh'] for x in kws)), '其他')
        for lang, line in zip(('zh', 'en'), STACK_TOP_WPN):
            if s.get(lang) is not None and line not in s[lang].split('\n'):
                s[lang] += '\n' + line
    report.append('叠加说明：新词条 %d 条加「%s」、%d 条加「%s」，武器特效 %d 种加「%s」；原包旧说法改了 %d 段' % (
        n['top'], STACK_TOP[0], n['same'], STACK_SAME[0], len(weapon['special']['pool']), STACK_TOP_WPN[0], n['old']))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--pack', default=os.path.join(PALY, 'packs', 'Ruby', 'recommended increased difficulty'))
    a = ap.parse_args()
    t0 = time.time()
    x = Extract(a.pack)
    gear = x.gear()
    x.closure({s for f in gear.values() for s in f['sp'].values() if s < NEW_SP})
    x.tier_triggers(gear)
    x.rescale(gear)
    x.rework(gear)
    x.load_speed(gear)
    x.high_hp(gear)
    x.item_dmg(gear)
    x.charged_spell(gear)
    x.hp_drain(gear)
    weapon = x.weapons()
    # 装备记录 / 存档里存的编号由 pin 按名字锁定（id_lock.json），步骤的先后不再影响它们；内部行每次整体重建
    hooks = x.rimeguard(gear)
    x.custom_vfx()
    x.custom(gear)
    x.rule_affixes(gear)
    x.crit_rage(gear)
    runtime = x.custom_shared(weapon)
    x.merge_and_balance(gear)
    stack_notes(gear, weapon, x.report)
    x.value_ranges(gear)
    x.sort_templates(gear)
    for p, rows in x.rows.items():
        clash = [i for i in rows if i in x.Pv[p].rows]
        if clash:
            raise RuntimeError('%s 新行和原版编号冲突：%s' % (p, clash[:10]))
    x.save_lock()
    lib = {
        '_说明': '词条库（阶段 1：迁移原包的词条）。由 dev\\item_system\\extract_affixes.py 从 RNG Lands 原包提取，不要手改；'
                 '规则见 dev\\item_system\\词条一览.md、设计文档.md 第 8 节。权重 / 分布里的数字都是原包里的件数。',
        'source': os.path.relpath(a.pack, PALY), 'time': time.strftime('%Y-%m-%d %H:%M:%S'),
        'rarities': list(RAR),
        'rarity_tag': {r: {'en': x.rar_tag[r][0], 'zh': x.rar_tag[r][1]} for r in RAR},
        'gear': gear,
        'hooks': hooks,
        'runtime': runtime,
        'armor': {'count': x.armor_count, 'slots': x.armor_slots, 'dlc_three_share': 0.3333},
        'weapon': weapon,
        'sell': {k: dict(zip(RAR, v)) for k, v in SELL.items()},
    }
    # 数值模型的词条强度折算（dev\power_curve\affix_power.json）：每个词条、每种武器特效都要写（"auto" / 折算 / {"none": 原因}），漏了就不写出词条库
    power = json.load(open(os.path.join(PALY, 'dev', 'power_curve', 'affix_power.json'), encoding='utf-8'))
    miss = [k for k in list(gear) + [s['key'] for s in weapon['special']['pool']] if k not in power]
    if miss:
        raise SystemExit('dev\\power_curve\\affix_power.json 里没写这些词条的强度折算（写 "auto"、折算或 {"none": 原因}）：\n  ' + '\n  '.join(miss))
    os.makedirs(OUT, exist_ok=True)
    json.dump(lib, open(os.path.join(OUT, 'affixes.json'), 'w', encoding='utf-8', newline='\n'), ensure_ascii=False, indent=1)
    rows = {p: {str(i): r.hex() for i, r in sorted(rs.items())} for p, rs in sorted(x.rows.items())}
    rows['_row_size'] = {p: x.Pv[p].row_size for p in x.rows}
    json.dump(rows, open(os.path.join(OUT, 'rows.json'), 'w', encoding='utf-8', newline='\n'), indent=0)
    x.report.insert(0, '词条库提取  %s  来源 %s  用时 %.1f 秒' % (lib['time'], lib['source'], time.time() - t0))
    x.report.append('护符 / 防具词条（合并后）：%d 种；分组：%s' % (len(gear), dict(Counter(f['group'] for f in gear.values()))))
    x.report.append('防具词条数分布：%s' % x.armor_count)
    x.report.append('武器类型（传说）：%s' % weapon['types']['Legendary'])
    x.report.append('补正项数：%s' % weapon['scaling']['count'])
    x.report.append('插进 regulation 的行：%s' % {p: len(r) for p, r in x.rows.items()})
    open(os.path.join(OUT, '提取报告.txt'), 'w', encoding='utf-8', newline='\n').write('\n'.join(x.report) + '\n')
    print('\n'.join(x.report))


if __name__ == '__main__':
    main()
