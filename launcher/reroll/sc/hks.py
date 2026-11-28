# -*- coding: utf-8 -*-
"""玩家脚本（原版 data/player/c0000.hks）和灵马脚本（data/torrent/c8000.hks）：
  不会摔死（no_fall_damage，所有模式）：去掉高处坠落死亡的判定（NO_FALL）；
  不受坠落伤害（只在重随版：效果行 features.json 的 no_fall_sp 由 rr/world.py 照抄 ELDEN VINS 建，骑马也算）：
    每帧的 Update() 里检查，没有就挂上（死亡 / 休息被清掉后也会马上补回）。这个效果带状态 266，原版落地处理看到 266
    就不分高度一律「中等落地」：改成落差超过 8 米（原版这里是重落地）才这样，8 米以内照原版（NO_FALL_EFFECT）；
  蹬墙跳（wall_jump，N 网 Modular Wall Jump，data/player/modules）：按作者说明在 global = {} 上面加载两个模块，
    WallJump.lua 写出时按 WALL_JUMP_FIX 修正（模块原文件不动）；
  灵马速度（torrent_speed）：Move_onGenerate 末尾加 act(SetMovementScaleMult, 倍数)，和 ELDEN VINS、Convergence 的做法一样"""
import os
import re

from .common import DATA

PLAYER = os.path.join(DATA, 'player', 'c0000.hks')
PLAYER_MODULES = os.path.join(DATA, 'player', 'modules')
TORRENT = os.path.join(DATA, 'torrent', 'c8000.hks')
PLAYER_REL = os.path.join('action', 'script', 'c0000.hks')
TORRENT_REL = os.path.join('action', 'script', 'c8000.hks')
# 关掉坠落伤害 / 摔死：(原文, 改成)，每处必须正好出现一次
NO_FALL = [
    ('    if is_enable_falling_death ~= TRUE or not (height >= 60) or env(GetStateChangeType, 266) == TRUE then\n',
     '    if true or is_enable_falling_death ~= TRUE or not (height >= 60) or env(GetStateChangeType, 266) == TRUE then -- Dark Reforged: no_fall_damage\n'),
    ('    elseif height > 20 and env(IsInvincibleDebugMode) == FALSE and env(GetStateChangeType, 266) == FALSE then\n',
     '    elseif false and height > 20 and env(IsInvincibleDebugMode) == FALSE and env(GetStateChangeType, 266) == FALSE then -- Dark Reforged: no_fall_damage\n'),
]


def no_fall_effect(sp):
    return [
        ('function Update()\n    GetConstVariable()\n',
         'function Update()\n'
         '    if env(GetSpEffectID, %d) == FALSE then act(AddSpEffect, %d) end -- Dark Reforged: no_fall_damage\n'
         '    GetConstVariable()\n' % (sp, sp)),
        ('        if env(GetStateChangeType, 266) == TRUE then\n            Replanning()\n            SetVariable("LandIndex", LAND_MIDDLE)\n',
         '        if env(GetStateChangeType, 266) == TRUE and height > 8 then -- Dark Reforged: no_fall_damage（小落差照原版落地）\n'
         '            Replanning()\n            SetVariable("LandIndex", LAND_MIDDLE)\n'),
    ]


WALL_JUMP_LOAD = ''.join(r'pcall(loadfile(debug.getinfo(1, "S").source:sub(2):match("(.*/)"):gsub("/", "\\") .. "modules\\%s"))' % m + '\n\n'
                         for m in ('REQS.lua', 'WallJump.lua'))
# 蹬墙跳模块的修正：CanWJ 每帧只要在空中，就调原版的 ExecJumpMagic / ExecFallMagic / ExecFallAttack 当「正在跳跃施法 / 跳劈吗」的判断，
# 但这几个函数有输入就直接出招（全身事件、一离地就能出、跳劈中还能再出一次）。去掉这三个调用，只留两个纯判断：出招交回原版
WALL_JUMP_FIX = [('ExecJumpMagic(jump_type) == FALSE and ExecFallMagic() == FALSE and IS_ATTACKED_JUMPMAGIC == FALSE and ExecFallAttack() == FALSE and ',
                  'IS_ATTACKED_JUMPMAGIC == FALSE and ')]


# 新词条（rr/affixes.py 把词条库里 runtime 是这几种的写进 features.json 的 affix_hks：标记效果和数值，都是 普通 ~ 传说 4 个）：
#   gale 疾风：Move_onUpdate 末尾的 SetMovementScaleMult 乘上 1 + 数值%（同名叠加：各件数值相加，副本见 rr/affixes.py STACK_NEW_KEYS）；
#   wolf 狼魂：普通攻击 / 战技 / 架势 / 致命一击的公共函数里，防御取消改调 DR_GuardCancel：不在取消时机时，攻击中新按下防御也能取消，扣数值点精力
#     （按键看 ActionDuration 原始按住时间，ActionRequest 在取消时机外读不到）；
#   leap 飞鸿：ExecJump 扣完跳跃精力后挂计时标记（dll\dark_core 在标记期间设翻滚同款的无敌位）；
#   overlord 霸王：一次性霸体。所有战技状态（SwordArts*、拔刀架势 DrawStance* 的 _onUpdate）开头每帧挂霸体效果
#     （0.3 秒，照忍耐的图标、光效、减伤），第一次会硬直的受击在 ExecDamage 里按「无硬直」，之后这次战技不再有霸体；
#   事件总线 DR_Event（dll\dark_core 看到脉冲标记才做）：翻滚中、换弹 / 拉弓中、攻击开始、受击、握法（grip1 / grip2 持续状态，style 切换），
#     监听表由重随按词条定义写进 DR_AFFIX 的 ev_* 项，只在戴着在听的词条时挂
AFFIX_HELPERS = '''-- Dark Reforged: 新词条（launcher\\reroll\\sc\\hks.py，数值和效果编号来自词条库）
DR_AFFIX = %s
function DR_AffixTier(ids)
    if ids == nil then
        return 0
    end
    for i = 4, 1, -1 do
        if env(GetSpEffectID, ids[i]) == TRUE then
            return i
        end
    end
    return 0
end

-- 疾风：同名叠加（2026-11-15）。每个稀有度：标记在就算一件，再数 dark_core 按件数挂的副本（gale_cp，每档 gale_n 个，按顺序挂，数到没有的就停），
-- 件数 × 这一档的数值相加。没有副本表（旧的重随）就照以前只取最高稀有度
function DR_MoveMult()
    local ids = DR_AFFIX.gale
    if ids == nil then
        return 1
    end
    local cp = DR_AFFIX.gale_cp
    if cp == nil then
        local t = DR_AffixTier(ids)
        if t > 0 then
            return 1 + DR_AFFIX.gale_v[t] / 100
        end
        return 1
    end
    local per = DR_AFFIX.gale_n[1]
    local pct = 0
    for i = 1, 4 do
        if env(GetSpEffectID, ids[i]) == TRUE then
            local n = 1
            local more = TRUE
            for j = 1, per do
                local c = cp[(i - 1) * per + j]
                if more == TRUE and c > 0 and env(GetSpEffectID, c) == TRUE then
                    n = n + 1
                else
                    more = FALSE
                end
            end
            pct = pct + n * DR_AFFIX.gale_v[i]
        end
    end
    return 1 + pct / 100
end

-- 狼魂：ActionRequest 只在动作的取消时机认按键（引擎按 TAE 屏蔽），所以看防御键的原始按住时间 ActionDuration。
-- 只认攻击开始后松开过、再新按的防御：DR_WolfSeen = 这一帧在攻击类状态里调过 DR_GuardCancel，
-- Update 每帧看一次，上一帧不在攻击里就清掉 DR_WolfReady（按着防御出的攻击，比如防御反击，不会马上被取消）
DR_WolfSeen = FALSE
DR_WolfReady = FALSE
function DR_WolfFrame()
    if DR_WolfSeen == FALSE then
        DR_WolfReady = FALSE
    end
    DR_WolfSeen = FALSE
end

-- 日志标记（wolf_*_mark，dll\\dark_core 看到写日志）：atk 攻击类状态里调到了、van 原版取消时机防御了、try 尝试防御、ok 防御成功
function DR_GuardCancel(guardcondition, blend_type)
    if ExecGuardOnCancelTiming(guardcondition, blend_type) == TRUE then
        DR_Pulse("wolf_van_mark", "wolf")
        return TRUE
    end
    local t = DR_AffixTier(DR_AFFIX.wolf)
    if t == 0 then
        return FALSE
    end
    DR_WolfSeen = TRUE
    DR_Mark("wolf_atk_mark")
    if env(ActionDuration, ACTION_ARM_L1) <= 0 then
        DR_WolfReady = TRUE
        return FALSE
    end
    if DR_WolfReady == FALSE or env(IsGuardFromAtkCancel) == TRUE then
        return FALSE
    end
    DR_Mark("wolf_try_mark")
    local ok = FALSE
    if guardcondition == TO_GUARDON then
        ok = ExecGuard(Event_GuardOn, blend_type)
    else
        ok = ExecGuard(Event_GuardStart, blend_type)
    end
    if ok == TRUE then
        DR_Mark("wolf_ok_mark")
        DR_WolfReady = FALSE
        if DR_AFFIX.wolf_v[t] > 0 then
            AddStamina(-DR_AFFIX.wolf_v[t])
        end
    end
    return ok
end

function DR_JumpIFrames()
    DR_Event("jump")
    local t = DR_AffixTier(DR_AFFIX.leap)
    if t > 0 then
        act(AddSpEffect, DR_AFFIX.leap_sp[t])
    end
end

-- 霸王：一次性霸体。战技状态里每帧挂霸体效果（光效、图标、减伤；效果本身不改受击等级），
-- 效果挂着时第一次会硬直的受击（受击等级不是「无」，或出血 / 冻伤发作）在 ExecDamage 里按「无硬直」处理（只播不打断动作的叠加受击），
-- 然后这次战技的霸体就用掉了：不再挂效果，挂受击标记（dll\\dark_core 看到马上摘掉效果、写日志），之后的受击照常硬直。
-- DR_ArmorSeen = 这一帧在战技状态里；Update 每帧 DR_ArmorFrame：上一帧不在战技里就清掉 DR_ArmorUsed（下次战技重新有霸体），
-- 连着接的战技（中间没离开战技状态）算同一次。DR_ArmorHitNow = 这一帧挡过，同一帧再调 ExecDamage 也按无硬直
DR_ArmorSeen = FALSE
DR_ArmorUsed = FALSE
DR_ArmorHitNow = FALSE
function DR_ArmorFrame()
    if DR_ArmorSeen == FALSE then
        DR_ArmorUsed = FALSE
    end
    DR_ArmorSeen = FALSE
    DR_ArmorHitNow = FALSE
end

function DR_SkillArmor()
    DR_Event("skill") -- 事件「战技中」（乘势：战技中受击不掉层、战技结束用掉层、满层霸体）
    local t = DR_AffixTier(DR_AFFIX.overlord)
    if t > 0 then
        DR_ArmorSeen = TRUE
        if DR_ArmorUsed == FALSE then
            act(AddSpEffect, DR_AFFIX.overlord_sp[t])
            DR_Event("armor") -- 霸体在：dll\\dark_core 挂忍耐的图标、光效（不限时，不在了就摘，免得 0.3 秒的效果图标一直闪）
        end
    end
end

function DR_SkillArmorOn()
    local ids = DR_AFFIX.overlord_sp
    if ids == nil then
        return FALSE
    end
    for i = 1, 4 do
        if env(GetSpEffectID, ids[i]) == TRUE then
            return TRUE
        end
    end
    return FALSE
end

-- 这一下是不是出血 / 冻伤 / 发狂发作：受击等级是「无」时原版会升级成小硬直（发狂同出血，用户 2026-11-09）；序号同抗性模块：2 出血、4 冻伤、6 发狂
function DR_StatusProc()
    if env(GetDamageSpecialAttribute, 2) == TRUE or env(GetDamageSpecialAttribute, 4) == TRUE or env(GetDamageSpecialAttribute, 6) == TRUE then
        return TRUE
    end
    return FALSE
end

function DR_DamageLevel()
    local lv = env(GetDamageLevel)
    -- 相杀：动作播到那一段时受击不会被打断，出防御火花（减伤在效果行里，dll\\dark_core 按武器的防御时减伤率改）
    if DR_ClashOn() == TRUE then
        -- ExecDamage 每帧都跑：火花只在这一帧真的挨了打时放
        if env(HasReceivedAnyDamage) == TRUE then
            DR_ClashSpark()
        end
        if lv ~= DAMAGE_LEVEL_NONE or DR_StatusProc() == TRUE then
            DR_ArmorHitNow = TRUE
            return DAMAGE_LEVEL_NONE
        end
    end
    if DR_ArmorHitNow == TRUE then
        return DAMAGE_LEVEL_NONE
    end
    -- 浮空：起跳后（落地前）第一次会硬直的受击按无硬直；dll\\dark_core 挂着浮空的效果就是还没用掉，发 float_hit 让它摘
    if DR_AnyOn("float_on") == TRUE and (lv ~= DAMAGE_LEVEL_NONE
        or DR_StatusProc() == TRUE) then
        DR_ArmorHitNow = TRUE
        DR_Event("float_hit")
        return DAMAGE_LEVEL_NONE
    end
    -- 强韧：喝红露滴圣杯瓶时和喝完后几秒（dll\\dark_core 挂着它的效果），会硬直的受击都按无硬直（不是一次性的）；
    -- 乘势：满 3 层时使出的战技期间同样（沉稳以前按住蓄力时也是，2026-11-05 改成缩短蓄力时间、改名寸劲）
    if (DR_AnyOn("endure_hold") == TRUE or DR_AnyOn("endure_after") == TRUE
        or DR_AnyOn("momentum_full") == TRUE) and (lv ~= DAMAGE_LEVEL_NONE
        or DR_StatusProc() == TRUE) then
        DR_ArmorHitNow = TRUE
        return DAMAGE_LEVEL_NONE
    end
    if DR_ArmorUsed == FALSE and DR_SkillArmorOn() == TRUE and (lv ~= DAMAGE_LEVEL_NONE
        or DR_StatusProc() == TRUE) then
        DR_ArmorUsed = TRUE
        DR_ArmorHitNow = TRUE
        DR_Mark("overlord_hit_mark")
        return DAMAGE_LEVEL_NONE
    end
    return lv
end

-- 重击蓄力（重击处决）：按住重击的状态（Attack*Heavy*Start）每帧调。蓄力中（TAE 的蓄力标志，或原版效果 100280）发 charging；
-- 蓄力段过完还在这个状态 = 没中途松开、满蓄力放出，之后每帧发 charged（中途松开会转到 Heavy1End 出招，不发）。
-- 每次重新进这个状态要从「没蓄力过」算起：Update 每帧加 DR_Frame，上一帧没调过就是刚进来；换了状态（连招里满蓄力的 Heavy1Start
-- 直接接 Heavy2Start，帧是连着的）也算刚进来（用户 2026-11-20：以前满蓄力带进下一下的起手，下一下蓄力前就发 charged，千钧当成同一击不再扣）
DR_Frame = 0
DR_HeavySeen = -1
DR_HeavyState = ""
DR_Charged = FALSE
function DR_HeavyFrame(st)
    if (DR_HeavySeen ~= DR_Frame - 1 and DR_HeavySeen ~= DR_Frame) or DR_HeavyState ~= st then
        DR_Charged = FALSE
    end
    DR_HeavySeen = DR_Frame
    DR_HeavyState = st
    if env(GetGeneralTAEFlag, TAE_FLAG_CHARGING) == 1 or env(GetSpEffectID, 100280) == TRUE then
        DR_Charged = TRUE
        DR_Event("charging")
    elseif DR_Charged == TRUE then
        DR_Event("charged")
    end
end

-- 相杀（用户 2026-11-04）：轻击 / 重击的状态每帧调，发 clash_<l|h|hc>_<r|l>（dll\\dark_core 按当前动作的攻击判定前后挂相杀的效果）。
-- kind：l 轻击、h 重击（松手出招、冲刺 / 防反等）、s 重击的起手蓄力状态（按住蓄力时不发；满蓄力出手后发 hc）；
-- left：用左手武器（左手攻击、双持；双手握左手武器也算左手）
function DR_ClashFrame(kind, left, both)
    DR_Event("swing") -- 事件「普通攻击中」（轻 / 重 / 跳跃 / 冲刺 / 防反；乘势：这时打中才加层）
    local hand = "r"
    if left == TRUE or c_Style == HAND_LEFT_BOTH then
        hand = "l"
    end
    if kind == "s" then
        if env(GetGeneralTAEFlag, TAE_FLAG_CHARGING) == 1 or env(GetSpEffectID, 100280) == TRUE or DR_Charged ~= TRUE then
            return
        end
        DR_Event("clash_hc_" .. hand) -- 这一下实际出招的手（武器特效来源判定也用它）
        return
    end
    -- 先发左右手攻击提示：即使没有相杀词条，武器特效也要靠它判断这一下是哪只手打的。
    -- both = 双持同时攻击：左右手的武器特效都算这一下生效。
    if both == TRUE then
        DR_Event("clash_" .. kind .. "_l")
        DR_Event("clash_" .. kind .. "_r")
    else
        DR_Event("clash_" .. kind .. "_" .. hand)
    end
    if DR_AFFIX.clash_lr == nil then
        return
    end
end

-- 跳跃攻击：JumpAttackForm 1 = 轻击、2 = 重击、3 = 双持轻击；拿着弓 / 弩 / 法杖的不算（那是射击、施法）
-- 跳跃轻击的事件和重击同一个，分不出来，这里按「上一帧不在跳跃轻击里」算新的一下轻击（血狂）
DR_JumpLightFrame = -10
function DR_ClashJumpFrame()
    local hand = HAND_RIGHT
    if c_Style == HAND_LEFT_BOTH then
        hand = HAND_LEFT
    end
    if GetEquipType(hand, WEAPON_CATEGORY_SMALL_ARROW, WEAPON_CATEGORY_ARROW, WEAPON_CATEGORY_LARGE_ARROW, WEAPON_CATEGORY_CROSSBOW,
        WEAPON_CATEGORY_BALLISTA, WEAPON_CATEGORY_STAFF) == TRUE then
        return
    end
    local f = GetVariable("JumpAttackForm")
    if f == 2 then
        DR_ClashFrame("h", FALSE)
    elseif f == 1 or f == 3 then
        if DR_Frame - DR_JumpLightFrame > 1 then
            DR_Event("light_new")
        end
        DR_JumpLightFrame = DR_Frame
        DR_ClashFrame("l", f == 3, f == 3)
    end
end

-- 相杀的效果（轻击 / 重击 × 右手 / 左手）挂着没有
function DR_ClashOn()
    if DR_AnyOn("clash_lr") == TRUE or DR_AnyOn("clash_hr") == TRUE or DR_AnyOn("clash_ll") == TRUE or DR_AnyOn("clash_hl") == TRUE then
        return TRUE
    end
    return FALSE
end

-- 相杀受击：防御火花，挂在正在相杀的那只手的武器护手处，每次受击放一次。
-- 一次受击「受到伤害」会连着亮好几帧，每帧都清掉重挂的话特效一直从头开始、几乎看不见（用户 2026-11-04「特效还是没触发」）：
-- 只在连着受击的第一帧放（上一帧、这一帧已经调过就不放）
DR_ClashSparkFrame = -10
function DR_ClashSpark()
    local last = DR_ClashSparkFrame
    DR_ClashSparkFrame = DR_Frame
    if last == DR_Frame or last == DR_Frame - 1 then
        return
    end
    local ids = DR_AFFIX.clash_spark_r
    if DR_AnyOn("clash_ll") == TRUE or DR_AnyOn("clash_hl") == TRUE then
        ids = DR_AFFIX.clash_spark_l
    end
    if ids == nil then
        return
    end
    for i = 1, #ids do
        if ids[i] > 0 then
            act(ClearSpEffect, ids[i])
            act(AddSpEffect, ids[i])
            return
        end
    end
end

-- 积木词条导出的效果（DR_AFFIX[名字] = 各稀有度的效果）身上有没有
function DR_AnyOn(name)
    local ids = DR_AFFIX[name]
    if ids == nil then
        return FALSE
    end
    for i = 1, #ids do
        if ids[i] > 0 and env(GetSpEffectID, ids[i]) == TRUE then
            return TRUE
        end
    end
    return FALSE
end

-- 每帧的状态事件：在空中（原版判断落地用的 IsLanding；dll\\dark_core 没收到 air 就算在地上——物理模块的「站在地上」标记在空中也是真，
-- 不能用）、跳跃攻击中（空中攻击和落地攻击：JumpAttackForm 不是 0，落地攻击做完才清零）
-- 凌空（用户 2026-10-29 重新设计「命中后接跳」）：跳跃攻击命中后 dll\\dark_core 挂 0.5 秒「可以接跳」的效果（air_ready），
-- 挂着时按跳跃键立刻起跳，发 air_jump（引擎拿走「可以接跳」、连跳加一层）和 jump（浮空、天堑等）。按键用 ActionDuration 自己找刚按下
-- （ActionRequest 受取消时机限制，落地攻击的后摇里读不到）。在地上（落地攻击 / 后摇）直接起跳；还在空中攻击时先清掉攻击的变量、
-- 切到下落 Jump_Loop（原版攻击中也会发这个），下一帧再起跳（还在起跳段 Jump_N/F/D 时直接发起跳是自己跳到自己，不生效）
DR_JumpHeld = 0
DR_AirJumpQueued = FALSE
function DR_AirJumpFrame()
    if env(IsLanding) == FALSE then
        DR_Event("air")
    end
    if GetVariable("JumpAttackForm") ~= 0 then
        DR_Event("jatk")
    end
    local held = env(ActionDuration, ACTION_ARM_CHANGE_STYLE)
    local pressed = held > 0 and DR_JumpHeld <= 0
    DR_JumpHeld = held
    if DR_AirJumpQueued == TRUE then
        DR_AirJumpQueued = FALSE
        DR_AirJump()
        return
    end
    if pressed == false or DR_AnyOn("air_ready") == FALSE or env(GetStamina) <= 0 then
        return
    end
    if env(IsLanding) == TRUE then
        DR_AirJump()
        return
    end
    SetVariable("JumpAttackForm", 0)
    SetVariable("JumpAttack_Land", 0)
    IS_ATTACKED_JUMPMAGIC = FALSE
    hkbFireEvent("W_Jump_Loop")
    DR_AirJumpQueued = TRUE
end

function DR_AirJump()
    DR_Event("air_jump")
    DR_Event("jump")
    if env(GetSpEffectID, 102360) == FALSE then
        AddStamina(STAMINA_REDUCE_JUMP) -- 照原版起跳扣精力（天堑补回）
    end
    act(LockonFixedAngleCancel)
    if c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH then
        SetVariable("JumpAttack_HandCondition", 1)
    else
        SetVariable("JumpAttack_HandCondition", 0)
    end
    SetVariable("JumpAttackForm", 0)
    SetVariable("JumpUseMotion_Bool", false)
    SetVariable("JumpMotion_Override", 0.009999999776482582)
    SetVariable("JumpAttack_Land", 0)
    SetVariable("SwingPose", 0)
    IS_ATTACKED_JUMPMAGIC = FALSE
    if GetVariable("MoveSpeedLevel") >= 0.6000000238418579 then
        SetVariable("JumpDirection", 0)
        SetVariable("JumpAngle", 0)
        SetVariable("IsEnableDirectionJumpTAE", true)
        ExecEvent("W_Jump_F")
    else
        SetVariable("JumpReachSelector", 0)
        ExecEvent("W_Jump_N")
    end
end

function DR_Mark(mark)
    if DR_AFFIX[mark] ~= nil then
        act(AddSpEffect, DR_AFFIX[mark][1])
    end
end

function DR_Pulse(mark, need)
    if DR_AFFIX[mark] ~= nil and DR_AffixTier(DR_AFFIX[need]) > 0 then
        DR_Mark(mark)
    end
end

-- 戴着这条词条吗（积木词条的标记表按词条键放在 DR_AFFIX 里）
function DR_Has(key)
    if DR_AffixTier(DR_AFFIX[key]) > 0 then
        return TRUE
    end
    return FALSE
end

-- 事件总线（dll\\dark_core 的积木引擎读）：DR_AFFIX["ev_名字"] = {脉冲标记, 在听这个事件的词条的标记…}（重随按词条定义生成）；
-- 身上戴着任意一条在听的词条，就挂这个事件的脉冲标记。没人听的事件什么都不做
function DR_Event(name)
    local e = DR_AFFIX["ev_" .. name]
    if e == nil then
        return
    end
    for i = 2, #e do
        if env(GetSpEffectID, e[i]) == TRUE then
            act(AddSpEffect, e[1])
            return
        end
    end
end

-- 握法：每帧发持续状态 grip1（单手）/ grip2（双手），握法变了发 style（0 = 还没记过；不能用 nil，见 DR_NEW_TABLES）
DR_Grip = 0
function DR_GripFrame()
    local two = c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH
    if two then
        DR_Event("grip2")
    else
        DR_Event("grip1")
    end
    if DR_Grip ~= 0 and DR_Grip ~= two then
        DR_Event("style")
    end
    DR_Grip = two
end

-- 施法 cast / 射击 shot / 投掷 throw：这一类状态里每帧发 kind（持续状态，近身靠 cast 排除法术打中）；新的一次发 kind_new
-- （凝聚加法球、迸发射出、无尽箭袋扣专注）。新的一次 = 发起它的事件（文件最后包的 ExecEventHalfBlend / ExecEventAllBody，DR_NewKind），
-- 或者这一类状态上一帧没在跑、这一帧开始跑（兜底：事件名对不上的）。连着的同类状态（放出 → 蓄力放出、射出 → 边走边射）不算新的
DR_StateSeen = {}
function DR_StateFrame(kind)
    local seen = DR_StateSeen[kind]
    if seen ~= DR_Frame - 1 and seen ~= DR_Frame then
        DR_Event(kind .. "_new")
    end
    DR_StateSeen[kind] = DR_Frame
    DR_Event(kind)
end

-- 骑马用道具的「一次性动作」状态大家共用：照这次的道具动作（IndexItemUseAnim，见 ExecRideItem）认投掷道具
function DR_IsThrowAnim(t)
    return t == ITEM_THROW_KNIFE or t == ITEM_THROW_BOTTLE or t == ITEM_THROW_SPEAR or t == ITEM_STRING_BOTTLE or t == ITEM_THROW_BIGBOTTLE
end

function DR_RideThrowFrame()
    if DR_IsThrowAnim(GetVariable("IndexItemUseAnim")) then
        DR_StateFrame("throw")
    end
end

'''
# 事件「攻击开始」：包一层 ExecAttack，返回真（出招了）就发；接在文件最后（这时原来的 ExecAttack 已经定义）
AFFIX_ATTACK_WRAP = '''
-- Dark Reforged（launcher\\reroll\\sc\\hks.py）：事件「攻击开始」
DR_ExecAttack_Orig = ExecAttack
function ExecAttack(r1, r2, l1, l2, b1, b2, is_guard, blend_type, artsr1, artsr2, is_stealth_rolling)
    local ret = DR_ExecAttack_Orig(r1, r2, l1, l2, b1, b2, is_guard, blend_type, artsr1, artsr2, is_stealth_rolling)
    if ret == TRUE then
        DR_Event("attack")
    end
    return ret
end
'''
# 事件「新的一次施法 / 射击 / 投掷」cast_new / shot_new / throw_new：发起它们的事件当帧发（同一个状态里接着再来一次的也算：
# 连续施法、连着投飞刀；施法不含蓄力放出 Cancel、起手 Launch、蓄力 Loop）。半身同步下半身用的是 ExecEventHalfBlendNoReset（LOWER），不经过这里。
# 认法：事件名（ExecEventAllBody 的字符串、事件表的第一个）或者事件表本身（原版事件表在公共定义里，第一次用到时才查，没有的跳过）。
# 第一个 %s = 事件名 → 类别，第二个 %s = DR_NewTable 那几行（affix_patch 用 new_events 生成）
AFFIX_NEW_WRAP = '''
-- Dark Reforged（launcher\\reroll\\sc\\hks.py）：事件「新的一次施法 / 射击 / 投掷」
DR_NEW_EVENTS = {%s}
-- 不能用 nil 当「还没建」：文件末尾原版的 _G 元表让没定义的全局读出 dummy 函数（没定义的 Event_X 也是 dummy）
DR_NEW_TABLES = false
function DR_NewTable(t, kind)
    if t ~= nil and t ~= dummy then
        DR_NEW_TABLES[t] = kind
    end
end
function DR_NewKind(event)
    if DR_NEW_TABLES == false then
        DR_NEW_TABLES = {}
%s
    end
    local kind = DR_NEW_TABLES[event]
    if kind == nil then
        kind = DR_NEW_EVENTS[event]
    end
    return kind
end
DR_ExecEventHalfBlend_Orig = ExecEventHalfBlend
function ExecEventHalfBlend(event_table, blend_type)
    if blend_type ~= LOWER then
        local kind = DR_NewKind(event_table)
        if kind == nil then
            kind = DR_NEW_EVENTS[event_table[1]]
        end
        if kind ~= nil then
            DR_Event(kind .. "_new")
        end
    end
    return DR_ExecEventHalfBlend_Orig(event_table, blend_type)
end
DR_ExecEventAllBody_Orig = ExecEventAllBody
function ExecEventAllBody(event)
    local kind = DR_NEW_EVENTS[event]
    if kind ~= nil then
        DR_Event(kind .. "_new")
    elseif event == "W_RideItemOneShot" and DR_IsThrowAnim(GetVariable("IndexItemUseAnim")) then
        -- 骑马投掷（喝药、摇铃也走这个事件，所以不进 DR_NEW_EVENTS）：ExecRideItem 刚把这次的道具动作写进 IndexItemUseAnim，
        -- 发起这一刻就发「新的一次投掷」，和地上投掷同一帧（以前等进了状态的第一帧才发，马上投壶照样消耗，用户 2026-11-10）
        DR_StateFrame("throw")
    end
    return DR_ExecEventAllBody_Orig(event)
end
'''
# 新的一次射击 / 投掷：发起它们的事件（原版事件表 Event_<名> 和事件名 W_<名> 都认；施法的从施法状态的函数名生成，见 cast_events）
NEW_EVENTS = {
    'shot': ('AttackArrowRightFireMove', 'AttackArrowLeftFireMove', 'AttackArrowRightFireStep', 'AttackArrowLeftFireStep',
             'AttackCrossbowRightFire', 'AttackCrossbowLeftFire', 'AttackCrossbowBothRightFire', 'AttackCrossbowBothLeftFire',
             'StealthAttackCrossbowRightFire', 'StealthAttackCrossbowLeftFire', 'StealthAttackCrossbowBothRightFire',
             'StealthAttackCrossbowBothLeftFire', 'RideAttackArrowFire', 'RideAttackCrossbowLeftFire', 'SwordArtsArrowFire'),
    'throw': ('ItemThrowKnife', 'ItemThrowBottle', 'ItemThrowSpear', 'ItemThrowBackBottle', 'ItemThrowBigBottle',
              'QuickItemThrowKnifeNormal', 'QuickItemThrowKnifeDash', 'QuickItemThrowKnifeStep', 'QuickItemThrowKnifeAttackRight',
              'QuickItemThrowKnifeAttackLeft', 'QuickItemThrowKnifeAttackRight2', 'QuickItemThrowKnifeAttackLeft2',
              'RideItemQuick1', 'RideItemQuick2', 'RideItemQuick3'),
    # 新的一下轻击（血狂，用户 2026-11-06「统一改成轻击时提升」）：范围同相杀的轻击（AFFIX_STATE_PULSE 的 l），原版没有事件的几个状态不列；跳跃轻击在 DR_ClashJumpFrame
    'light': tuple('AttackRightLight%d' % i for i in range(1, 7)) + tuple('AttackBothLight%d' % i for i in range(1, 7))
             + tuple('AttackDualLight%d' % i for i in range(1, 7)) + tuple('AttackLeftLight%d' % i for i in range(1, 7))
             + ('AttackRightLightSubStart', 'AttackBothLightSubStart', 'AttackDualLightSubStart',
                'AttackRightLightStep', 'AttackRightLightStealth', 'AttackRightLightDash', 'AttackRightWhileGuard',
                'AttackRightSpecial1', 'AttackRightSpecial2', 'AttackRightBackstep',
                'AttackBothSpecial1', 'AttackBothSpecial2', 'AttackBothDash', 'AttackBothLightStep', 'AttackBothLightStealth',
                'AttackBothBackstep', 'AttackBothLightGuard', 'AttackDualDash', 'AttackDualRolling', 'AttackDualStealth', 'AttackDualBackStep',
                'AttackBothLeft1', 'AttackBothLeft2', 'AttackBothLeft3', 'AttackBothLeftSpecial1', 'AttackBothLeftSpecial2',
                'AttackBothLeftSpecial3', 'AttackBothLeftSpecial4', 'AttackBothLeftSpecial5'),
}


def new_events(s):
    """{类别: [名字…]}：施法从施法状态的函数名来（cast_events），射击 / 投掷照 NEW_EVENTS；每个名字要在原版脚本里出现过（Event_<名> 或 "W_<名>"）"""
    out = {'cast': [e[2:] for e in cast_events(s)]}
    for kind, names in NEW_EVENTS.items():
        miss = [n for n in names if not re.search(r'\bEvent_%s\b' % n, s) and '"W_%s"' % n not in s]
        if miss:
            raise RuntimeError('玩家脚本里没有这些发起%s的事件：%s' % ({'shot': '射击', 'throw': '投掷', 'light': '轻击'}[kind], miss))
        out[kind] = list(names)
    return out


def cast_events(s):
    """施法状态里「新的一次施法」的事件名：W_ + 状态名（去掉 _Upper），不含蓄力放出（…Cancel、…Cancel2 / 3）"""
    names = set()
    for line in s.split('\n'):
        m = re.match(CAST_STATES, line.rstrip())
        if m:
            base = m.group(1)
            base = base[:-len('_Upper')] if base.endswith('_Upper') else base
            if not re.search(r'Cancel\d?$', base):
                names.add('W_' + base)
    return sorted(names)


# Update 里每帧：事件「受击」、握法；狼魂的「新按防御」判定、霸王的一次性霸体
# 换气（用户 2026-11-09）：ExecWeaponChange 是所有切换武器（左右手、站立 / 潜行）的入口，真的要切了才走到这里；只在双手共持时发事件
AFFIX_WSWITCH = ('    local event = Event_WeaponChangeStart\n',
                 '    if c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH then DR_Event("wswitch2") end -- Dark Reforged: 事件「双手共持时切换武器」\n'
                 '    local event = Event_WeaponChangeStart\n')
# 跳劈落地（原地跳的那几处只设变量、不进落地状态）：后面加一行发 jland（坠星）
JLAND_FIX = re.compile(r'^( +)SetVariable\("JumpAttack_Land", ([12])\)\n', re.M)
JLAND_SUB = r'\1SetVariable("JumpAttack_Land", \2)\n\1DR_Event("jland") -- Dark Reforged: 事件「跳跃攻击落地」（原地跳）\n'
JLAND_COUNT = 6
AFFIX_HIT = ('    SetStyleSpecialEffect()\n    act(SwitchMotion, FALSE)\n',
             '    SetStyleSpecialEffect()\n'
             '    DR_Frame = DR_Frame + 1 -- Dark Reforged: 帧数（重击蓄力判断刚进状态）\n'
             '    if env(HasReceivedAnyDamage) == TRUE then DR_Event("hit") end -- Dark Reforged: 事件「受击」\n'
             '    DR_GripFrame() -- Dark Reforged: 握法\n'
             '    DR_WolfFrame() -- Dark Reforged: 狼魂\n'
             '    DR_ArmorFrame() -- Dark Reforged: 霸王\n'
             '    DR_AirJumpFrame() -- Dark Reforged: 凌空\n'
             '    act(SwitchMotion, FALSE)\n')
# 状态函数开头发事件：(函数名的正则, 一行)
# 施法状态（2026-10-29 用户：连续施法还是不出法球——以前只认 MagicFire(Right|Left) 和跳跃 / 冲刺 / 后撤那几个，连续施法的第 2 / 3 下
# 走 MagicFireRight2 / 3、蓄力放出走 MagicFireRightCancel，都没有发施法）
CAST_STATES = r'^function ((Quick)?MagicFire(Right|Left)\w*_Upper|RollingMagic(Right|Left)_Upper|StealthMagic(Right|Left)Fire_Upper|RideMagicFire\w*)_onUpdate\(\)$'
# 施法全程（用户 2026-11-06 星群按法球加快施法）：CAST_STATES 只有放出，这里再加起手（Launch）、蓄力（Loop）
CASTING_STATES = r'^function ((Quick)?Magic(Fire|Launch|Loop)(Right|Left)\w*_Upper|RollingMagic(Right|Left)_Upper|StealthMagic(Right|Left)(Fire|Launch)_Upper|RideMagic(Fire|Launch|Loop)\w*)_onUpdate\(\)$'
AFFIX_STATE_PULSE = [
    (r'^function SwordArtsOneShot(ComboEnd)?_onUpdate\(\)$', '    DR_RamOn = TRUE -- Dark Reforged: 雷电羊球、雷云化身按住一直放（这两个状态能接）'),
    (r'^function Rolling(_Selftrans)?_onUpdate\(\)$', '    DR_Event("roll") -- Dark Reforged: 事件「翻滚中」'),
    (r'^function AttackCrossbow\w*Reload\w*_onUpdate\(\)$', '    DR_Event("reload") -- Dark Reforged: 事件「换弹 / 拉弓中」'),
    (r'^function AttackArrow\w*Start\w*_onUpdate\(\)$', '    DR_Event("reload") -- Dark Reforged: 事件「换弹 / 拉弓中」'),
    # 喝药：潜行 / 梯子 / 骑马的喝药状态红蓝瓶共用，按道具动作分（10 = 红露滴圣杯瓶，EquipParamGoods.goodsUseAnim；28 = 灵药）
    (r'^function (Stealth|Ride)?Item(Ladder)?Drinking(Right|Left|_Upper)?_onUpdate\(\)$',
     '    if env(GetItemAnimType) == 10 then DR_Event("flask") end -- Dark Reforged: 事件「喝红露滴圣杯瓶」'),
    (r'^function (Ride)?Item(Ladder)?Elixir2?(Right|Left|_Upper)?_onUpdate\(\)$', '    DR_Event("physick") -- Dark Reforged: 事件「喝灵药（持续状态）」'),
    # 蹲着喝灵药没有专门的状态：ExecStealthItem 把灵药（ITEM_ELIXIR = 28，只有灵药圣杯瓶用）和别的道具一样交给单次使用状态，
    # 按道具动作分（用户 2026-11-13「无敌没有覆盖到蹲着喝灵药」）。骑马喝走 RideItemElixir，上面那条已经认
    (r'^function StealthItemOneShot(_SelfTrans)?_Upper_onUpdate\(\)$',
     '    if env(GetItemAnimType) == ITEM_ELIXIR or env(GetItemAnimType) == ITEM_ELIXIR2 then DR_Event("physick") end '
     '-- Dark Reforged: 事件「喝灵药（持续状态）」（蹲着）'),
    # 施法：魔法 / 祷告放出来的所有状态（CAST_STATES，含连续施法的 2 / 3 段、蓄力放出的 Cancel、跳跃 / 冲刺 / 后撤 / 翻滚 / 潜行 / 骑马施法）；
    # 蹲下不用钩子，dll\dark_core 看原版潜行效果 8001。每帧发 cast（持续状态），新的一次施法另发 cast_new（见 DR_StateFrame、AFFIX_NEW_WRAP）
    (CAST_STATES, '    DR_StateFrame("cast") -- Dark Reforged: 事件「施法」'),
    (CASTING_STATES, '    DR_Event("casting") -- Dark Reforged: 事件「施法中」（含起手、蓄力）'),
    (r'^function DefaultBackStep_onUpdate\(\)$', '    DR_Event("backstep") -- Dark Reforged: 事件「后撤步」'),
    (r'^function Jump_(LandAttack_\w+|Attack_Land_F)_onUpdate\(\)$', '    DR_Event("jland") -- Dark Reforged: 事件「跳跃攻击落地」'),
    # 重击处决：致命一击（背刺、弹反处决、崩架势处决都是 Throw 状态）；按住重击蓄力（DR_HeavyFrame 发 charging / charged）
    (r'^function Throw(Atk|BackStab)_onUpdate\(\)$', '    DR_Event("crit") -- Dark Reforged: 事件「致命一击」'),
    # 处决、被抓的动作和对方同步，这时不能变速（处刑、致命舞、转念；dark_core 的 runtime.rs）
    (r'^function Throw(Atk|BackStab|Def|Escape)_onUpdate\(\)$', '    DR_Event("sync") -- Dark Reforged: 事件「同步动作」'),
    # 带上状态名：连招从一个蓄力重击状态直接接到下一个（Heavy1Start → Heavy2Start）时也从「没蓄力过」算起（用户 2026-11-20 千钧连续蓄力不扣）
    (r'^function (Attack(Right|Both)Heavy(Special)?[12]Start)_onUpdate\(\)$',
     lambda m: '    DR_HeavyFrame("%s") -- Dark Reforged: 事件「重击蓄力 / 满蓄力」' % m.group(1)),
    # 相杀（用户 2026-11-04）：轻击 / 重击（含冲刺、翻滚 / 后撤、跳跃、防御反击，不含战技、处决、射击），各状态每帧发 clash_<l|h|hc>_<r|l>
    (r'^function Attack(Right|Both)Heavy(Special)?[12]Start_onUpdate\(\)$', '    DR_ClashFrame("s", FALSE) -- Dark Reforged: 相杀（满蓄力出手后）'),
    (r'^function Attack(Right|Both)(Heavy[12]End|HeavyDash|HeavyCounter|HeavySpecial[12]End)_onUpdate\(\)$',
     '    DR_ClashFrame("h", FALSE) -- Dark Reforged: 相杀（重击）'),
    (r'^function AttackLeftHeavy\d_onUpdate\(\)$', '    DR_ClashFrame("h", TRUE) -- Dark Reforged: 相杀（左手重击）'),
    # 左手单独轻击（左右手武器不同类，原版的 L1；同类是 AttackDual*）：只算左手。以前归在双持里，右手武器的特效也跟着算（用户 2026-11-20「只算左手」）
    (r'^function AttackLeftLight1_onUpdate\(\)$', '    DR_ClashFrame("l", TRUE) -- Dark Reforged: 相杀（左手轻击）'),
    (r'^function Attack(Right|Both)(Light[1-6]|LightDash|LightStep|LightFastStep|LightStealth|LightCounter|LightGuard|Backstep|Dash|Special[12]|WhileGuard)_onUpdate\(\)$',
     '    DR_ClashFrame("l", FALSE) -- Dark Reforged: 相杀（轻击）'),
    (r'^function (AttackBothLeft(\d|Dash|Step|Special\d)|AttackDual(Light\d|Dash|BackStep|Rolling|Stealth))_onUpdate\(\)$',
     '    DR_ClashFrame("l", TRUE, TRUE) -- Dark Reforged: 相杀（双持轻击）'),
    # 起跳后马上按攻击（上升段，原版 SpEffect 140）：攻击叠在起跳状态 Jump_N / F / D 里播，下落切 Jump_Loop，原地跳连落地动作也在 Jump_N 里，
    # 不进 JumpAttack_Start_Falling（用户 2026-11-27「带燃血护符进行跳跃攻击正常消耗绿条」）→ 这四个状态也挂；DR_ClashJumpFrame 只在 JumpAttackForm 1–3 时发
    (r'^function (JumpAttack_Start_Falling(_F|_D)?|Jump_LandAttack_(Normal|Hard)|Jump_Attack_Land_F|Jump_(N|F|D|Loop))_onUpdate\(\)$',
     '    DR_ClashJumpFrame() -- Dark Reforged: 相杀（跳跃攻击）'),
    # 远程与道具：射出箭 / 弩箭（含潜行、骑马、战技的射出）、投掷道具（投掷壶、附绳壶、大投掷壶、飞刀、投矛，含攻击中快投飞刀、骑马投）、调香。
    # 射击、投掷每帧发 shot / throw，新的一次另发 shot_new / throw_new（无尽箭袋按这个扣专注；2026-10-29 以前只认弓、不认弩和快投）
    (r'^function ((Stealth)?Attack(Arrow|Crossbow)\w*Fire\w*|RideAttack(Arrow|Crossbow)\w*Fire|SwordArtsArrowFire)_onUpdate\(\)$',
     '    DR_StateFrame("shot") -- Dark Reforged: 事件「射箭」'),
    (r'^function ((Quick)?ItemThrow\w+_Upper|RideItemQuick\d)_onUpdate\(\)$', '    DR_StateFrame("throw") -- Dark Reforged: 事件「投掷道具」'),
    (r'^function RideItemOneShot_onUpdate\(\)$', '    DR_RideThrowFrame() -- Dark Reforged: 事件「投掷道具」（骑马）'),
    (r'^function ItemAroma\w+_Upper_onUpdate\(\)$', '    DR_Event("aroma") -- Dark Reforged: 事件「调香」'),
]
AFFIX_MOVE = ('''    if hkbGetVariable("MoveDirection") == 3 or hkbGetVariable("MoveDirection") == 2 then
        act(SetMovementScaleMult, 0.9599999785423279)
    elseif hkbGetVariable("MoveDirection") == 1 then
        act(SetMovementScaleMult, 0.9599999785423279)
    elseif hkbGetVariable("MoveDirection") == 0 then
        act(SetMovementScaleMult, 0.9800000190734863)
    end
end

function MoveNoSync_onActivate()''', '''    if hkbGetVariable("MoveDirection") == 3 or hkbGetVariable("MoveDirection") == 2 then
        act(SetMovementScaleMult, 0.9599999785423279 * DR_MoveMult()) -- Dark Reforged: 疾风
    elseif hkbGetVariable("MoveDirection") == 1 then
        act(SetMovementScaleMult, 0.9599999785423279 * DR_MoveMult())
    elseif hkbGetVariable("MoveDirection") == 0 then
        act(SetMovementScaleMult, 0.9800000190734863 * DR_MoveMult())
    elseif DR_MoveMult() > 1 then
        act(SetMovementScaleMult, DR_MoveMult())
    end
end

function MoveNoSync_onActivate()''')
# 霸王：受击等级改从 DR_DamageLevel 取；这一帧霸体挡下了，出血 / 冻伤发作也不升级成小硬直（原版只豁免忍耐 1650 本身）
# 壁垒防反：事件「防住攻击」guard_hit（有防御硬直的）、guard_light（轻的，没有硬直）、「防御反击」guard_counter；
# 不动如山（dr_unshaken）：破防（包括吹飞 / 击飞倒地的破防）都改成防御挨重击
AFFIX_DAMAGE = [('function ExecDamage(is_parry, is_attackwhileguard)\n    local damage_level = env(GetDamageLevel)\n',
                 'function ExecDamage(is_parry, is_attackwhileguard)\n    local damage_level = DR_DamageLevel() -- Dark Reforged: 霸王\n'),
                ('env(GetSpEffectID, 1650) == TRUE', 'env(GetSpEffectID, 1650) == TRUE or DR_ArmorHitNow == TRUE'),
                ('        if guard_damage_level > 0 then\n',
                 '        if guard_damage_level > 0 then\n            DR_Event("guard_hit") -- Dark Reforged: 事件「防住攻击」\n'),
                ('        else\n            if env(GetSpEffectID, 102001) == TRUE or env(GetSpEffectID, 102011) == TRUE or env(GetSpEffectID, 102013)\n'
                 '                == TRUE or env(GetSpEffectID, 102015) == TRUE then\n                SetJustGuardSucceedEffect(1)\n                ExecAddDamage(',
                 '        else\n            DR_Event("guard_light") -- Dark Reforged: 事件「防住轻攻击」（没有防御硬直）\n'
                 '            if env(GetSpEffectID, 102001) == TRUE or env(GetSpEffectID, 102011) == TRUE or env(GetSpEffectID, 102013)\n'
                 '                == TRUE or env(GetSpEffectID, 102015) == TRUE then\n                SetJustGuardSucceedEffect(1)\n                ExecAddDamage('),
                ('        elseif damage_type == DAMAGE_TYPE_GUARDBREAK then\n            if is_parry == TRUE then\n',
                 '        elseif damage_type == DAMAGE_TYPE_GUARDBREAK or (DR_Has("dr_unshaken") == TRUE and (damage_type == DAMAGE_TYPE_GUARDBREAK_BLAST\n'
                 '            or damage_type == DAMAGE_TYPE_GUARDBREAK_FLING)) then -- Dark Reforged: 不动如山（吹飞 / 击飞倒地的破防也在这里换掉）\n'
                 '            if is_parry == TRUE then\n'),
                ('            if c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH then\n                ExecEventAllBody("W_AttackBothHeavyCounter")\n',
                 '            DR_Event("guard_counter") -- Dark Reforged: 事件「防御反击」\n'
                 '            if c_Style == HAND_RIGHT_BOTH or c_Style == HAND_LEFT_BOTH then\n                ExecEventAllBody("W_AttackBothHeavyCounter")\n'),
                ('            SetVariable("IndexGuard", guardindex)\n            act(SetDamageAnimType, DAMAGE_FLAG_GUARD_BREAK)\n',
                 '            SetVariable("IndexGuard", guardindex)\n'
                 '            if DR_Has("dr_unshaken") == TRUE then -- Dark Reforged: 不动如山（不会被破防，改成防御挨重击）\n'
                 '                act(SetDamageAnimType, DAMAGE_FLAG_GUARD_EXLARGE)\n'
                 '                ExecEventAllBody("W_GuardDamageLarge")\n'
                 '                DR_Event("guard_hit")\n'
                 '                return TRUE\n'
                 '            end\n'
                 '            act(SetDamageAnimType, DAMAGE_FLAG_GUARD_BREAK)\n')]
AFFIX_JUMP = ('''        AddStamina(STAMINA_REDUCE_JUMP)
    end
    SetWeightIndex()
''', '''        AddStamina(STAMINA_REDUCE_JUMP)
    end
    DR_JumpIFrames() -- Dark Reforged: 飞鸿
    SetWeightIndex()
''')
WOLF_FUNCS = ('AttackCommonFunction', 'ArtsCommonFunction', 'ArtsStanceCommonFunction', 'BackStabCommonFunction')
# 战灰「唤声船」（rr/mariner.py）：技艺类型是它时按战技键不放动作（这个类型没有动画），挂请求标记，dll\dark_core 看到就扣蓝上船
BOAT_ANCHOR = '        if c_IsEnableSwordArts == TRUE then\n            local swordartrequest = GetSwordArtsRequestNew()\n'
BOAT_CHECK = ('        if c_IsEnableSwordArts == TRUE and c_SwordArtsID == %d then -- Dark Reforged: 唤声船\n'
              '            act(AddSpEffect, %d)\n'
              '            return ATTACK_REQUEST_INVALID\n'
              '        end\n')
# 坐在船上（用户 2026-10-29：坐在王座上，参考唤声船 Boss；上下船要有过渡）：船的效果挂着时放坐姿手势（坐下的动作就是上船的过渡，
# 之后一直循环坐着；骑船时 DLL 关掉了移动和动作，手势不会被按键打断）。下船时 DLL 挂标记：起身标记 → 放手势的起身动作，
# 跳船标记 → 从座位上起跳（同 ExecJump 原地跳那一支）；被打 / 死亡下船时不挂，照受击动作走。人物被 DLL 抬在座位高度，
# 船的效果挂着时不进坠落（ExecFallStart 开头加一条）
BOAT_HELPERS = '''-- Dark Reforged: 战灰「唤声船」坐在船上（launcher\\reroll\\sc\\hks.py）
DR_BOAT = {sps = {%s}, jump = %d, stand = %d, gesture = %d}
DR_BoatSit = FALSE
-- 帧数、最后一次在坐姿手势状态里（GestureLoopStart / GestureLoop 的状态函数记）、最后一次发坐下：坐下被别的动作挡掉时隔几帧再发
DR_BoatTick = 0
DR_BoatGestureAt = -100
DR_BoatSitAt = -100
function DR_BoatOn()
    for _, sp in ipairs(DR_BOAT.sps) do
        if env(GetSpEffectID, sp) == TRUE then
            return TRUE
        end
    end
    return FALSE
end
function DR_BoatJump()
    local style = c_Style
    if style == HAND_RIGHT then
        SetVariable("JumpAttack_HandCondition", 0)
    else
        SetVariable("JumpAttack_HandCondition", 1)
    end
    SetVariable("JumpAttackForm", 0)
    SetVariable("JumpUseMotion_Bool", false)
    SetVariable("JumpMotion_Override", 0.009999999776482582)
    SetVariable("JumpAttack_Land", 0)
    SetVariable("SwingPose", 0)
    SetVariable("JumpReachSelector", 0)
    IS_ATTACKED_JUMPMAGIC = FALSE
    ExecEvent("W_Jump_N")
end
function DR_BoatFrame()
    DR_BoatTick = DR_BoatTick + 1
    if env(GetSpEffectID, DR_BOAT.jump) == TRUE then
        if DR_BoatSit == TRUE then
            DR_BoatSit = FALSE
            DR_BoatJump()
        end
        return
    end
    if env(GetSpEffectID, DR_BOAT.stand) == TRUE then
        if DR_BoatSit == TRUE then
            DR_BoatSit = FALSE
            ExecEventHalfBlend(Event_GestureEnd, ALLBODY)
        end
        return
    end
    if DR_BoatOn() == TRUE then
        -- 刚上船，或者坐下的事件被别的动作挡掉了（4 帧以上不在坐姿手势里）：每 6 帧再发一次
        if DR_BoatSit == FALSE or (DR_BoatTick - DR_BoatGestureAt > 4 and DR_BoatTick - DR_BoatSitAt > 6) then
            DR_BoatSit = TRUE
            DR_BoatSitAt = DR_BoatTick
            SetVariable("GestureID", DR_BOAT.gesture)
            ExecEventHalfBlend(Event_GestureLoopStart, ALLBODY)
        end
    elseif DR_BoatSit == TRUE then
        -- 船没了（被打 / 传送下船）人还在坐姿里（没有受击动作接上）：站起来，不然要等玩家按键才起身
        DR_BoatSit = FALSE
        if DR_BoatTick - DR_BoatGestureAt <= 2 then
            ExecEventHalfBlend(Event_GestureEnd, ALLBODY)
        end
    end
end

'''
# 坐姿手势：坐在船上时不让按键 / 摇杆把手势结束（原版循环手势一有输入、一推摇杆就起身，骑船时一直在推摇杆转向，2026-10-29 用户：
# 坐下有时能触发有时不能）；受击等被动动作照旧。两个坐姿状态里记帧数，DR_BoatFrame 看是不是真的坐着
BOAT_GESTURE = [('function GestureLoopCommonFunction(blend_type, lower_state, is_start)\n'
                 '    if ExecPassiveAction(FALSE, FALL_TYPE_DEFAULT, FALSE) == TRUE then\n        return TRUE\n    end\n',
                 'function GestureLoopCommonFunction(blend_type, lower_state, is_start)\n'
                 '    if ExecPassiveAction(FALSE, FALL_TYPE_DEFAULT, FALSE) == TRUE then\n        return TRUE\n    end\n'
                 '    if DR_BoatOn() == TRUE then -- Dark Reforged: 唤声船（坐在船上时按键、摇杆不让起身）\n        return FALSE\n    end\n'),
                ('function GestureLoopStart_Upper_onUpdate()\n',
                 'function GestureLoopStart_Upper_onUpdate()\n    DR_BoatGestureAt = DR_BoatTick -- Dark Reforged: 唤声船（坐着）\n'),
                ('function GestureLoop_Upper_onUpdate()\n',
                 'function GestureLoop_Upper_onUpdate()\n    DR_BoatGestureAt = DR_BoatTick -- Dark Reforged: 唤声船（坐着）\n')]
BOAT_UPDATE = ('\nfunction Update()\n    GetConstVariable()\n',
               '\nfunction Update()\n    GetConstVariable()\n    DR_BoatFrame() -- Dark Reforged: 唤声船（坐在船上、下船）\n')
BOAT_NOFALL = ('function ExecFallStart(fall_type)\n    if env(IsFalling) == FALSE then\n        return FALSE\n    end\n',
               'function ExecFallStart(fall_type)\n    if env(IsFalling) == FALSE then\n        return FALSE\n    end\n'
               '    if DR_BoatOn() == TRUE then -- Dark Reforged: 唤声船（坐在船上时人物被抬在座位高度，不算坠落）\n        return FALSE\n    end\n')
# 霸王：战技状态函数名的开头（DrawStance = 拔刀架势那一类战技，也调战技的公共函数）
SKILL_STATES = ('function SwordArts', 'function DrawStance')


def lua_table(d):
    """{名字: [数]} → Lua 表"""
    return '{' + ', '.join('%s = {%s}' % (k, ', '.join(repr(x) for x in v)) for k, v in sorted(d.items())) + '}'


# 无形 dr_unseen（用户 2026-11-06「只管远程箭和法术道具 近战不屏蔽」「喝药灵药也不拦截」）：戴着时不通知 AI 的只有施法（NotifyAIMagicCast）、
# 蓄力施法（NotifyAIOfBehaviourState 的 CHARGEMAGIC）。AINotifyAttackType 的 FINDATTACK（只有近战普攻和跳劈才发，弓弩、战技的分支原版就关掉了）、
# USEITEM（喝药、灵药）和起跳都不动。箭、法术、投掷道具的子弹在 dark_core unseen.rs。人形 NPC 也用这份脚本，它们身上没有标记，照常通知。
UNSEEN_OFF = 'if DR_Has("dr_unseen") == FALSE then %s end -- Dark Reforged: 无形（不通知敌人）'
AFFIX_UNSEEN = [
    ('act(NotifyAIMagicCast)', UNSEEN_OFF % 'act(NotifyAIMagicCast)', 3),
    ('act(NotifyAIOfBehaviourState, IDX_AINOTE_STATETYPE, IDX_AINOTE_STATETYPE_CHARGEMAGIC)',
     UNSEEN_OFF % 'act(NotifyAIOfBehaviourState, IDX_AINOTE_STATETYPE, IDX_AINOTE_STATETYPE_CHARGEMAGIC)', 2),
]


# 雷电羊球（战技类型 113，动作段 a713）按住战技键一直滚（用户 2026-11-09）。原版连段：40010（ComboEnd 状态）带窗口效果 100052，窗口里再按战技 = 再放一次 ComboEnd
# （GetSwordArtsRequestNew 的 COMBO_1：SetSwordArtsPointInfo(R2) + W_SwordArtsOneShotComboEnd，能自己接自己）。这里把「再按」换成「按着」，第一下 OneShot（40000）没有这个窗口，
# 能移动取消时接过去。放在 ArtsCommonFunction 的翻滚之后：受击、换武器、翻滚照样能打断，又抢在移动取消之前（不然推着摇杆就断了）。
# 只有按下第一下消耗专注值（第一下走 L2 的费用，接着滚走 R2 的费用，rr/tweaks.py lightning_ram 把 R2 改成 0），所以这里不看专注值。
# 精力要看（用户 2026-11-20「雷电羊球没精力了还能造成伤害」）：每一下的攻击扣精力（行为 18 / 25），原版没精力时再按接不上；
# 这里按着就接，精力 ≤ 0 也照接、攻击照样出伤害。现在精力 ≤ 0 不再接，这一下滚完就停（雷云化身同）
# 雷云化身（类型 226，动作段 a826，用户 2026-11-09「也和雷电羊球一样改造」）：这一段没有连段动作（40010 不存在，放 ComboEnd 没有动作），OneShot 状态也不能自己接自己，
# 所以按着时播到能移动取消（或播完）先回待机，下一帧在 Idle_onUpdate 里重放（DR_CloudReplay）。动作开头先数几帧，免得开头的取消窗口马上重放。
# 原版「持续发动」在窗口 100285 里松手走 40001（短突刺），按着走完整的飞行，这一段照旧。费用同雷电羊球：第一下走 L2，重放走 R2（rr/tweaks.py 改成 0）
RAM_HOLD_HELPER = '''-- Dark Reforged: 雷电羊球、雷云化身按住战技键一直放（DR_RamOn：这一帧在能接的状态里）
DR_RamOn = FALSE
DR_CloudAgain = FALSE
DR_CloudFrames = 0
function DR_CloudReplay()
    local again = DR_CloudAgain
    DR_CloudAgain = FALSE
    DR_CloudFrames = 0
    if again == FALSE or c_SwordArtsID ~= 226 or env(ActionDuration, ACTION_ARM_L2) <= 0 or env(GetStamina) <= 0 then
        return FALSE
    end
    SetSwordArtsPointInfo(ACTION_ARM_R2, TRUE)
    ExecEventAllBody("W_SwordArtsOneShot")
    ClearAttackQueue()
    return TRUE
end

function DR_RamHold()
    local on = DR_RamOn
    DR_RamOn = FALSE
    if on == FALSE or env(ActionDuration, ACTION_ARM_L2) <= 0 or env(GetStamina) <= 0 then
        return FALSE
    end
    if c_SwordArtsID == 226 then
        DR_CloudFrames = DR_CloudFrames + 1
        if DR_CloudFrames < 8 or (env(IsMoveCancelPossible) == FALSE and env(IsAnimEnd, 0) == FALSE) then
            return FALSE
        end
        DR_CloudAgain = TRUE
        ExecEventAllBody("W_Idle")
        return TRUE
    end
    if c_SwordArtsID ~= 113 then
        return FALSE
    end
    if env(GetSpEffectID, 100052) == FALSE and env(IsMoveCancelPossible) == FALSE then
        return FALSE
    end
    SetSwordArtsPointInfo(ACTION_ARM_R2, TRUE)
    ExecEventAllBody("W_SwordArtsOneShotComboEnd")
    return TRUE
end

'''
RAM_HOLD_CALL = '    if DR_RamHold() == TRUE then -- Dark Reforged: 雷电羊球按住一直滚\n        ClearAttackQueue()\n        return TRUE\n    end\n'
RAM_HOLD_ANCHOR = '    if ExecItem(QUICKTYPE_ATTACK, blend_type) == TRUE then\n'
CLOUD_IDLE_ANCHOR = '\nfunction Idle_onUpdate()\n'
CLOUD_IDLE_CALL = '    if DR_CloudReplay() == TRUE then -- Dark Reforged: 雷云化身按住一直放\n        return\n    end\n'


def affix_patch(s, a):
    """新词条的钩子（a = features.json 的 affix_hks）"""
    k = s.index('\nfunction Update()\n')
    s = s[:k + 1] + AFFIX_HELPERS % lua_table(a) + RAM_HOLD_HELPER + s[k + 1:]
    i = s.index('\nfunction ArtsCommonFunction(')
    c = s.index(RAM_HOLD_ANCHOR, i)
    if c > s.index('\nfunction ', i + 1):
        raise RuntimeError('玩家脚本的 ArtsCommonFunction 里找不到要改的地方（雷电羊球）')
    s = s[:c] + RAM_HOLD_CALL + s[c:]
    if s.count(CLOUD_IDLE_ANCHOR) != 1:
        raise RuntimeError('玩家脚本里找不到待机状态（雷云化身）')
    s = s.replace(CLOUD_IDLE_ANCHOR, CLOUD_IDLE_ANCHOR + CLOUD_IDLE_CALL)
    for old, new in [AFFIX_MOVE, AFFIX_JUMP, AFFIX_HIT, AFFIX_WSWITCH] + AFFIX_DAMAGE:
        if s.count(old) != 1:
            raise RuntimeError('玩家脚本里找不到要改的地方（新词条）：' + old.strip()[:60])
        s = s.replace(old, new)
    for old, new, n in AFFIX_UNSEEN:
        if s.count(old) != n:
            raise RuntimeError('玩家脚本里通知 AI 的地方数量变了（无形）：' + old.strip()[:60])
        s = s.replace(old, new)
    # 坠星（用户 2026-11-12「坠星为什么连续跳劈时没有触发」）：原地跳（jump_type 0）的跳劈落地不进单独的落地状态，只在跳劈状态里
    # 设 JumpAttack_Land = 1 / 2 播落地动作，以前只有落地状态（Jump_LandAttack_* / Jump_Attack_Land_F）发 jland → 这几处也发
    s, n = JLAND_FIX.subn(JLAND_SUB, s)
    if n != JLAND_COUNT:
        raise RuntimeError('玩家脚本里跳劈落地的地方数量变了（坠星）：%d 处，应该 %d 处' % (n, JLAND_COUNT))
    for fn in WOLF_FUNCS:                       # 这几个公共函数里第一处防御取消
        i = s.index('\nfunction %s(' % fn)
        j = s.index('\nfunction ', i + 1)
        c = s.index('ExecGuardOnCancelTiming(', i)
        if c > j:
            raise RuntimeError('玩家脚本的 %s 里没有防御取消（新词条狼魂）' % fn)
        s = s[:c] + 'DR_GuardCancel(' + s[c + len('ExecGuardOnCancelTiming('):]
    n = 0
    pulses = [(re.compile(rx), line, [0]) for rx, line in AFFIX_STATE_PULSE]
    out = []
    for line in s.split('\n'):
        out.append(line)
        if line.startswith(SKILL_STATES) and line.rstrip().endswith('_onUpdate()'):
            out.append('    DR_SkillArmor() -- Dark Reforged: 霸王')
            n += 1
        for rx, add, cnt in pulses:
            m = rx.match(line.rstrip())
            if m:
                out.append(add(m) if callable(add) else add)
                cnt[0] += 1
    if n < 20:
        raise RuntimeError('玩家脚本里的战技状态只找到 %d 个（新词条霸王）' % n)
    for rx, add, cnt in pulses:
        if cnt[0] == 0:
            raise RuntimeError('玩家脚本里没找到要挂标记的状态：%s' % rx.pattern)
    if '\nfunction ExecAttack(' not in '\n' + '\n'.join(out):
        raise RuntimeError('玩家脚本里没有 ExecAttack（新词条血狂）')
    ev = new_events(s)
    if 'MagicFireRight' not in ev['cast'] or 'MagicFireRight2' not in ev['cast']:
        raise RuntimeError('玩家脚本的施法状态对不上（新的一次施法）：%s' % ev['cast'])
    names = ', '.join('["W_%s"] = "%s"' % (n, kind) for kind in sorted(ev) for n in ev[kind])
    tables = '\n'.join('        DR_NewTable(Event_%s, "%s")' % (n, kind) for kind in sorted(ev) for n in ev[kind])
    return '\n'.join(out) + AFFIX_ATTACK_WRAP + AFFIX_NEW_WRAP % (names, tables)


# 对勇于挑战者的馈赠（rr/curses.py；做法参考 Golden Age 的 Second Wind：死亡入口拦下、回满血、不进死亡状态）。
# 刚复活的效果（2 秒无敌）在身上时一律不算死：ChangeHP 还没写进去、或者这一下带着「致死」伤害类型，都别进死亡。
# 可用的效果由 dark_core 摘（它看到刚复活的效果才记「用过了」），这里不摘，万一下一帧还是 0 血就再来一次
REVIVE_HELPER = '''-- Dark Reforged: 对勇于挑战者的馈赠（高难度每次休息后第一次死亡满状态复活）
function DR_Revive()
    if env(GetSpEffectID, %(just)d) == TRUE then
        if env(GetHP) <= 0 then
            act(ChangeHP, 99999)
        end
        return TRUE
    end
    if env(GetHP) <= 0 and env(GetSpEffectID, %(ready)d) == TRUE then
        act(ChangeHP, 99999)
        act(AddSpEffect, %(just)d)
        return TRUE
    end
    return FALSE
end

-- Dark Reforged: 被投技打死（被抓着时游戏判定「这一下会死」，原版直接进投技死亡动作 ThrowDeath，不走上面的死亡入口）：
-- 复活还能用就不进 ThrowDeath，被抓的动作照常播完；血掉到 0 的那一帧照上面一样回满（判定可能在血到 0 之前就出来，所以有可用的就先拦下）
function DR_ReviveThrow()
    if DR_Revive() == TRUE then
        return TRUE
    end
    if env(GetSpEffectID, %(ready)d) == TRUE then
        return TRUE
    end
    return FALSE
end

'''
REVIVE_PATCH = [
    ('function ExecDeath()\n    if env(GetReceivedDamageType) == DAMAGE_TYPE_DEATH or env(GetHP) <= 0 then\n'
     '        if env(GetSpEffectID, 560) == TRUE then\n            return FALSE\n        end\n',
     'function ExecDeath()\n    if env(GetReceivedDamageType) == DAMAGE_TYPE_DEATH or env(GetHP) <= 0 then\n'
     '        if env(GetSpEffectID, 560) == TRUE then\n            return FALSE\n        end\n'
     '        if DR_Revive() == TRUE then\n            return FALSE\n        end\n'),
    ('function IsLandDead(height)\n    if env(GetHP) <= 0 then\n',
     'function IsLandDead(height)\n    if DR_Revive() == TRUE then\n        return FALSE\n    end\n    if env(GetHP) <= 0 then\n'),
    ('function ExecLadderDeath()\n    local hp = env(GetHP)\n',
     'function ExecLadderDeath()\n    if DR_Revive() == TRUE then\n        return FALSE\n    end\n    local hp = env(GetHP)\n'),
    ('function ExecRideDeath()\n',
     'function ExecRideDeath()\n    if DR_Revive() == TRUE then\n        return FALSE\n    end\n'),
    ('    if env(GetSpecialAttribute) == 25 and 0 >= env(GetHP) and env(IsOnMount) == TRUE then\n',
     '    if DR_Revive() ~= TRUE and env(GetSpecialAttribute) == 25 and 0 >= env(GetHP) and env(IsOnMount) == TRUE then\n'),
    ('    elseif env(GetHP) <= 0\n        and (env(GetSpecialAttribute) == DAMAGE_ELEMENT_POISON',
     '    elseif DR_Revive() ~= TRUE and env(GetHP) <= 0\n        and (env(GetSpecialAttribute) == DAMAGE_ELEMENT_POISON'),
    # 被投技打死（用户 2026-11-22「修一下死亡难度以上吃boss投技复活不生效的问题」）：ThrowDef 里判定致死直接进 ThrowDeath
    ('    if env(IsThrowSelfDeath) == TRUE then\n        ExecEvent("ThrowDeath")\n        return\n    end\n',
     '    if env(IsThrowSelfDeath) == TRUE and DR_ReviveThrow() ~= TRUE then\n        ExecEvent("ThrowDeath")\n        return\n    end\n'),
]


def revive_patch(s, ids):
    k = s.index('\nfunction Update()\n')
    s = s[:k + 1] + REVIVE_HELPER % {'ready': int(ids[0]), 'just': int(ids[1])} + s[k + 1:]
    for a, b in REVIVE_PATCH:
        if s.count(a) != 1:
            raise RuntimeError('玩家脚本里找不到要改的地方（战斗复活）：' + a.strip()[:60])
        s = s.replace(a, b)
    return s


# 发狂发作的硬直同出血（用户 2026-11-09，ailment_rework）：原版发狂在 ExecDamage 里单独放 W_DamageMad（骑马 W_RideDamageMad）整段动作，
# 改成和出血、冻伤走同一支：受击等级是「无」时升级成小硬直，忍耐和霸体也能挡。发作特效原来在那段动作的 TAE 里，这里挂（效果编号同 rr/ailments.py 的 MAD_VFX_SP）
MAD_VFX_SP = 7299110
MAD_THREE = 'if env(GetDamageSpecialAttribute, 2) == TRUE or env(GetDamageSpecialAttribute, 4) == TRUE or env(GetDamageSpecialAttribute, 6) == TRUE then -- Dark Reforged: 发狂发作的硬直同出血\n'
MAD_ADD = '        if env(GetDamageSpecialAttribute, 6) == TRUE then\n            act(AddSpEffect, %d) -- Dark Reforged: 发狂发作特效\n        end\n' % MAD_VFX_SP
MAD_TWO = 'if env(GetDamageSpecialAttribute, 2) == TRUE or env(GetDamageSpecialAttribute, 4) == TRUE then\n'
MADNESS_FLINCH = [('    if env(GetDamageSpecialAttribute, 6) == TRUE then\n        ExecEventAllBody("W_DamageMad")\n        return TRUE\n    end\n    ' + MAD_TWO,
                   '    ' + MAD_THREE + MAD_ADD),
                  ('    if env(GetDamageSpecialAttribute, 6) == TRUE then\n        FireRideEvent("W_RideDamageMad", "W_RideRun_End", FALSE)\n        Replanning()\n        return TRUE\n    end\n', ''),
                  ('    ' + MAD_TWO + '        if damage_level == DAMAGE_LEVEL_NONE then\n            damage_level = DAMAGE_LEVEL_SMALL\n',
                   '    ' + MAD_THREE + MAD_ADD + '        if damage_level == DAMAGE_LEVEL_NONE then\n            damage_level = DAMAGE_LEVEL_SMALL\n')]


# 咒痕「千变」（rr/curses.py，照 Golden Age 的多才多艺）：各招式的动作状态每帧挂这种招式的标记（只在咒痕的监听标记挂着时，也就是高难度档里）
VARY_HELPER = '''-- Dark Reforged: 咒痕「千变」的招式标记（rr/curses.py 的 VARY_STATES）
function DR_VaryMark(sp)
    if env(GetSpEffectID, %d) == TRUE then
        act(AddSpEffect, sp)
    end
end

'''


def vary_patch(s, listen, marks):
    """每个状态的 onUpdate 开头加一行 DR_VaryMark；找不到的状态跳过（不同游戏版本可能少几个），少于一半就报错"""
    n = 0
    for name, sp in marks.items():
        head = 'function %s_onUpdate()\n' % name
        if s.count(head) == 1:
            s = s.replace(head, head + '    DR_VaryMark(%d) -- Dark Reforged: 千变\n' % int(sp))
            n += 1
    if n < len(marks) // 2:
        raise RuntimeError('玩家脚本里只找到 %d / %d 个千变的招式状态' % (n, len(marks)))
    k = s.index('\nfunction Update()\n')
    return s[:k + 1] + VARY_HELPER % int(listen) + s[k + 1:]


def write_player(path, no_fall, wall_jump, sp=None, affix=None, boat=None, revive=None, madness=False, vary=None):
    s = open(PLAYER, encoding='utf-8-sig').read().replace('\r\n', '\n')
    if revive:
        s = revive_patch(s, revive)
    if affix:
        s = affix_patch(s, affix)
    if madness:
        for old, new in MADNESS_FLINCH:
            if s.count(old) != 1:
                raise RuntimeError('玩家脚本里找不到要改的地方（发狂硬直）：' + old.strip()[:60])
            s = s.replace(old, new)
    if boat:
        if s.count(BOAT_ANCHOR) != 1:
            raise RuntimeError('玩家脚本里找不到战技请求的地方（唤声船）')
        s = s.replace(BOAT_ANCHOR, BOAT_CHECK % (int(boat['arts_type']), int(boat['req_sp'])) + BOAT_ANCHOR)
        if boat.get('boat_sps'):
            for a, b in [BOAT_UPDATE, BOAT_NOFALL] + BOAT_GESTURE:
                if s.count(a) != 1:
                    raise RuntimeError('玩家脚本里找不到要改的地方（唤声船坐姿）：' + a.strip()[:60])
                s = s.replace(a, b)
            k = s.index('\nfunction Update()\n')
            s = s[:k + 1] + BOAT_HELPERS % (', '.join(str(int(x)) for x in boat['boat_sps']), int(boat['jump_sp']), int(boat['stand_sp']),
                                             int(boat['sit_gesture'])) + s[k + 1:]
    for a, b in ((NO_FALL + (no_fall_effect(sp) if sp else [])) if no_fall else []):
        if s.count(a) != 1:
            raise RuntimeError('玩家脚本里找不到要改的地方（游戏版本变了？）：' + a.strip()[:60])
        s = s.replace(a, b)
    if wall_jump:
        mark = '\nglobal = {}\n'
        if s.count(mark) != 1:
            raise RuntimeError('玩家脚本里找不到 global = {}')
        k = s.index(mark)
        s = s[:k + 1] + WALL_JUMP_LOAD + s[k + 1:]
        mdir = os.path.join(os.path.dirname(path), 'modules')
        os.makedirs(mdir, exist_ok=True)
        for m in ('REQS.lua', 'WallJump.lua'):
            raw = open(os.path.join(PLAYER_MODULES, m), 'rb').read()
            if m == 'WallJump.lua':
                t = raw.decode('utf-8')
                for a, b in WALL_JUMP_FIX:
                    if t.count(a) != 1:
                        raise RuntimeError('蹬墙跳模块里找不到要改的地方（模块换版本了？）：' + a[:60])
                    t = t.replace(a, b)
                raw = t.encode('utf-8')
            open(os.path.join(mdir, m), 'wb').write(raw)
    if vary:
        s = vary_patch(s, vary[0], vary[1])
    # 文件末尾原版的 _G 元表让没定义的全局读出 dummy 函数：全局设成 nil 再判断 == nil 永远不成立（2026-10-29 人物动不了）
    bad = re.findall(r'(?m)^DR_\w+ = nil\b', s)
    if bad:
        raise RuntimeError('玩家脚本里有全局设成 nil（读出来是 dummy 函数，改用 false / 0）：' + ', '.join(bad))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, 'w', encoding='utf-8', newline='').write(s)


def write_torrent(path, speed):
    lines = open(TORRENT, encoding='utf-8-sig').read().split('\n')
    k = next(i for i, l in enumerate(lines) if l.startswith('function Move_onGenerate'))
    end = next(i for i in range(k + 1, len(lines)) if lines[i].rstrip() == 'end')
    lines.insert(end, '    act(SetMovementScaleMult, %r) -- Dark Reforged: torrent_speed' % float(speed))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, 'w', encoding='utf-8', newline='').write('\n'.join(lines))


def files(cx, tmp):
    speed = float(cx.S.get('torrent_speed', 2.0))
    no_fall, wall_jump = bool(cx.S.get('no_fall_damage', True)), bool(cx.S.get('wall_jump', True))
    if speed != 1.0:
        write_torrent(os.path.join(tmp, TORRENT_REL), speed)
    affix, boat = cx.feat.get('affix_hks'), cx.feat.get('mariner_boat')
    cu = cx.feat.get('curses') or {}
    revive = cu.get('revive')
    vary = (cu['listen'], cu['vary']) if cu.get('vary') else None
    madness = bool((cx.S.get('ailment_rework') or {}).get('enabled', True))
    if no_fall or wall_jump or affix or boat or revive or madness or vary:
        write_player(os.path.join(tmp, PLAYER_REL), no_fall, wall_jump, cx.feat.get('no_fall_sp'), affix, boat, revive, madness, vary)


def sources(cx):
    return [PLAYER, TORRENT, os.path.join(PLAYER_MODULES, 'REQS.lua'), os.path.join(PLAYER_MODULES, 'WallJump.lua')]
