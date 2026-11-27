//! Dark Reforged 装备工坊的悬浮界面（dark_ui；用户 2026-10-31：自己画界面、单独一个 DLL、做好隔离；同天要求照原版商店的大小和素材重做）。
//! 赐福「装备工坊」（sc/inherit.py）→ dark_core 应答 → 这里每帧问 dark_core 的 dr_ws_state，该开就开窗口；关窗口告诉 dark_core，
//! 赐福菜单就回去了。数据（快照）、规则和执行都在 dark_core（gear\workshop.rs），这里只画、只读快照、只提请求，不碰游戏内存。
//! 界面和操作在 ui.rs（离线预览 bin\preview.rs 也用它），原版素材和物品图标在 assets.rs，字体 fonts.rs；这里是挂进游戏的部分：
//! 读手柄 / 键盘、拦游戏输入（hooks.rs，窗口开着时游戏收不到任何输入，关窗口后等按键松开才放行）、把图传成贴图、和 dark_core 来回。
//! 悬浮层用 hudhook（Dear ImGui，DirectX 12）。

pub mod assets;
pub mod data;
pub mod fonts;
mod hooks;
mod sound;
pub mod ui;

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use assets::{Assets, Rgba};
use data::{API, Api, Prev, Snap};
use hudhook::hooks::dx12::ImguiDx12Hooks;
use hudhook::imgui::{self, Ui};
use hudhook::windows::core::BOOL;
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};
use ui::{Img, P_DOWN, P_LEFT, P_RIGHT, P_UP, Tex, Workshop};

const STICK: i16 = 16000;
const REPEAT_DELAY: Duration = Duration::from_millis(350);
const REPEAT_EVERY: Duration = Duration::from_millis(90);
const RELEASE_MAX: Duration = Duration::from_secs(2);
/// 同时交给解码线程几张图；一帧最多花多久把解好的图传成贴图
const MAX_JOBS: usize = 8;
const UPLOAD_BUDGET: Duration = Duration::from_millis(3);

/// 预先要的原版素材（打开窗口时一起传，免得一张张冒出来）
const PRELOAD: [&str; 14] = [
    "MENU_BaseMain_01",
    "MENU_BaseMain_02",
    "MENU_TitleBase",
    "MENU_FL_MenuBase",
    "MENU_FL_Cursor_Wide",
    "MENU_Line_01",
    "MENU_SlotBase",
    "MENU_SL_Cursor",
    "MENU_FL_Rar_d0",
    "MENU_FL_Rar_d1",
    "MENU_FL_Rar_d2",
    "MENU_FL_Tab_Cursor",
    "KG_XboxSeries_R_D",
    "KG_XboxSeries_R_R",
];

static HMODULE: OnceLock<usize> = OnceLock::new();

/// 本地时间：加载那行写上日期和时间（每行开头是加载后的秒数），直接调 kernel32
#[repr(C)]
#[derive(Default)]
struct LocalTime {
    year: u16,
    month: u16,
    _weekday: u16,
    day: u16,
    hour: u16,
    min: u16,
    sec: u16,
    _ms: u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLocalTime(t: *mut LocalTime);
}

fn local_time() -> LocalTime {
    let mut t = LocalTime::default();
    unsafe { GetLocalTime(&mut t) };
    t
}

/// 运行日志 dll\dark_ui\dark_ui.log（每次加载重写）：加载、挂钩子、初始化、第一帧、找 dark_core 接口、开关窗口、panic。
/// 最多 300 行（每开关一次装备工坊记两行，玩得久会写满）；写满时记一行说明，之后只记 panic（再最多 100 行），免得后面出的错看不到
fn dlog(msg: &str) {
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;
    const MAX: usize = 300;
    static LINES: AtomicUsize = AtomicUsize::new(0);
    static T0: OnceLock<Instant> = OnceLock::new();
    let panic = msg.starts_with("panic");
    let n = LINES.load(Ordering::Relaxed);
    if n > MAX && (!panic || n > MAX + 100) {
        return;
    }
    let n = LINES.fetch_add(1, Ordering::Relaxed);
    let t = T0.get_or_init(Instant::now).elapsed().as_secs_f32();
    let path = root().join("dll").join("dark_ui").join("dark_ui.log");
    let f = if n == 0 { std::fs::File::create(&path) } else { std::fs::OpenOptions::new().append(true).open(&path) };
    if let Ok(mut f) = f {
        if n == MAX && !panic {
            let _ = writeln!(f, "[{t:8.2}] （日志写满 {MAX} 行，之后只记 panic）");
        } else {
            let _ = writeln!(f, "[{t:8.2}] {msg}");
        }
    }
}

/// hudhook 内部的警告 / 错误（tracing）也写进 dark_ui.log：同一处最多写 3 次（有的每帧报一次）
struct HudLog;

impl hudhook::tracing::Subscriber for HudLog {
    fn enabled(&self, m: &hudhook::tracing::Metadata<'_>) -> bool {
        *m.level() <= hudhook::tracing::Level::WARN
    }
    fn new_span(&self, _: &hudhook::tracing::span::Attributes<'_>) -> hudhook::tracing::span::Id {
        hudhook::tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &hudhook::tracing::span::Id, _: &hudhook::tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &hudhook::tracing::span::Id, _: &hudhook::tracing::span::Id) {}
    fn event(&self, e: &hudhook::tracing::Event<'_>) {
        struct V(String);
        impl hudhook::tracing::field::Visit for V {
            fn record_debug(&mut self, f: &hudhook::tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}{}", self.0);
                } else {
                    self.0 += &format!(" {}={v:?}", f.name());
                }
            }
        }
        static SEEN: std::sync::Mutex<Option<HashMap<String, u32>>> = std::sync::Mutex::new(None);
        let m = e.metadata();
        let at = format!("{}:{}", m.file().unwrap_or("?"), m.line().unwrap_or(0));
        {
            let mut g = SEEN.lock().unwrap_or_else(|x| x.into_inner());
            let n = g.get_or_insert_with(HashMap::new).entry(at.clone()).or_insert(0);
            *n += 1;
            if *n > 3 {
                return;
            }
        }
        let mut v = V(String::new());
        e.record(&mut v);
        dlog(&format!("hudhook {}（{at}）：{}", m.level(), v.0));
    }
    fn enter(&self, _: &hudhook::tracing::span::Id) {}
    fn exit(&self, _: &hudhook::tracing::span::Id) {}
}

/// 几个悬浮层（dark_ui、dark_dev）挂同一组 DirectX 12 钩子要排队：hudhook 在 with() 时就把目标函数的开头抄进跳板（MH_CreateHook），
/// apply() 才写跳转。两个 DLL 交错（A 抄、B 抄、A 写、B 写）时 B 的跳板里是原版开头，A 的钩子被绕过、也不报错，界面一帧都不画
/// （2026-10-31 两局赐福提示「没有加载界面」，日志只有「钩子挂上了」）。用同名互斥量排队（dark_dev 用同一个名字），最多等 30 秒
struct HookLock(*mut c_void);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(attr: *const c_void, initial: i32, name: *const u16) -> *mut c_void;
    fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
    fn ReleaseMutex(h: *mut c_void) -> i32;
    fn CloseHandle(h: *mut c_void) -> i32;
}

impl HookLock {
    fn take() -> Option<Self> {
        let name: Vec<u16> = "Local\\DarkReforged.D3D12Hooks".encode_utf16().chain([0]).collect();
        let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if h.is_null() {
            return None;
        }
        match unsafe { WaitForSingleObject(h, 30_000) } {
            0 | 0x80 => Some(Self(h)), // WAIT_OBJECT_0 / WAIT_ABANDONED（上一个拿着的线程没放就退了，也算拿到）
            _ => {
                unsafe { CloseHandle(h) };
                None
            }
        }
    }
}

impl Drop for HookLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

/// 界面初始化过（hudhook 第一次真的经过我们的钩子画东西）
static DRAWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// dark_reforged 根目录：本 DLL 在 dll\dark_ui\dark_ui.dll
fn root() -> PathBuf {
    use hudhook::windows::Win32::Foundation::HMODULE as HM;
    use hudhook::windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    let mut buf = [0u16; 1024];
    let h = HM(HMODULE.get().copied().unwrap_or(0) as _);
    let n = unsafe { GetModuleFileNameW(Some(h), &mut buf) } as usize;
    let p = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    p.ancestors().nth(3).map(|x| x.to_path_buf()).unwrap_or_default()
}

fn pad_buttons() -> u16 {
    let Some(s) = hooks::read_pad() else { return 0 };
    let mut b = s.buttons;
    if s.ly > STICK {
        b |= P_UP;
    }
    if s.ly < -STICK {
        b |= P_DOWN;
    }
    if s.lx < -STICK {
        b |= P_LEFT;
    }
    if s.lx > STICK {
        b |= P_RIGHT;
    }
    if s.lt > 100 {
        b |= ui::P_LT;
    }
    if s.rt > 100 {
        b |= ui::P_RT;
    }
    b
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum TKey {
    Sprite(String),
    Icon(u16),
}

/// 游戏里的图：要的时候还没传就记下来，后台线程读盘、解压（不卡画面），before_render 里传成贴图（解不出来的记 None，不再试）。
/// 画面上的先解，再解马上要滚进来的（余量）；这两份每帧重新记，已经滚走的就不解了
#[derive(Default)]
struct GameTex {
    cache: HashMap<TKey, Option<Img>>,
    /// 打开窗口时一起要的素材（不随帧清掉）
    boot: Vec<TKey>,
    want: Vec<TKey>,
    soon: Vec<TKey>,
    /// 交给解码线程、还没回来的
    busy: HashSet<TKey>,
    job: Option<Sender<TKey>>,
    done: Option<Mutex<Receiver<(TKey, Option<Rgba>)>>>,
}

impl GameTex {
    fn get(&mut self, k: TKey) -> Option<Img> {
        match self.cache.get(&k) {
            Some(x) => *x,
            None => {
                if !self.want.contains(&k) {
                    self.want.push(k);
                }
                None
            }
        }
    }

    /// 开解码线程
    fn start(&mut self, a: Arc<Assets>) {
        let (job, jobs) = channel::<TKey>();
        let (out, done) = channel();
        std::thread::spawn(move || {
            for k in jobs {
                let r = match &k {
                    TKey::Sprite(s) => a.sprite(s),
                    TKey::Icon(i) => a.icon(*i),
                };
                if out.send((k, r)).is_err() {
                    break;
                }
            }
        });
        self.job = Some(job);
        self.done = Some(Mutex::new(done));
    }

    /// hudhook 重建画面管线后，传好的贴图编号都属于旧贴图表，全部作废（不清的话画的时候编号越界）。
    /// 解码线程照常用；路上还没回来的解完照样传，传进新的贴图表
    fn reset(&mut self) {
        self.cache.clear();
    }

    /// 这一帧重新记要画的
    fn begin_frame(&mut self) {
        self.want.clear();
        self.soon.clear();
    }

    /// 解好的传成贴图（限时，传不完下一帧接着传），再派新的活
    fn pump(&mut self, render_context: &mut dyn RenderContext) {
        if let Some(rx) = &self.done {
            let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
            let t0 = Instant::now();
            while t0.elapsed() < UPLOAD_BUDGET {
                let Ok((k, rgba)) = rx.try_recv() else { break };
                self.busy.remove(&k);
                let img = rgba.and_then(|r| render_context.load_texture(&r.data, r.w, r.h).ok().map(|id| Img { id, w: r.w as f32, h: r.h as f32 }));
                self.cache.insert(k, img);
            }
        }
        let Some(job) = &self.job else { return };
        for k in self.boot.iter().chain(&self.want).chain(&self.soon) {
            if self.busy.len() >= MAX_JOBS {
                break;
            }
            if !self.busy.contains(k) && !self.cache.contains_key(k) {
                self.busy.insert(k.clone());
                let _ = job.send(k.clone());
            }
        }
        let cache = &self.cache;
        self.boot.retain(|k| !cache.contains_key(k));
    }
}

impl Tex for GameTex {
    fn sprite(&mut self, name: &str) -> Option<Img> {
        self.get(TKey::Sprite(name.to_string()))
    }

    fn icon(&mut self, id: u16) -> Option<Img> {
        self.get(TKey::Icon(id))
    }

    fn preload_icon(&mut self, id: u16) {
        let k = TKey::Icon(id);
        if !self.cache.contains_key(&k) && !self.soon.contains(&k) {
            self.soon.push(k);
        }
    }
}

struct App {
    api: Option<Api>,
    api_try: Instant,
    /// 钩子：(光标, 键鼠, 手柄)
    hooks: (usize, usize, bool),
    open: bool,
    /// 刚关窗口：手柄 / 键还没松开前照样挡着游戏
    release_wait: bool,
    closed_at: Instant,
    ws: Workshop,
    tex: GameTex,
    assets: Option<Arc<Assets>>,
    snap_ver: u32,
    prev_ver: u32,
    api_tries: u32,
    ver_logged: bool,
    res_ver: u32,
    pad_prev: u16,
    /// 开窗口时还按着工坊用的键盘键（比如选菜单的回车）：全松开以前不收键盘
    kb_wait: bool,
    rep_at: Option<Instant>,
    frame: u64,
    font: String,
    /// initialize 跑过几次（hudhook 重建画面管线会再跑）；上次 initialize 以后 before_render 走到过没有（没走到 = hudhook 没建成，
    /// 多半是字体图集建不出来）；连着几次没建成；字体退到第几步（fonts::add_at）
    inits: u32,
    ran: bool,
    init_fails: u32,
    font_step: u32,
}

impl App {
    fn new() -> Self {
        App {
            api: None,
            api_try: Instant::now() - Duration::from_secs(10),
            hooks: (0, 0, false),
            open: false,
            release_wait: false,
            closed_at: Instant::now(),
            ws: Workshop::default(),
            tex: GameTex::default(),
            assets: None,
            snap_ver: 0,
            prev_ver: 0,
            api_tries: 0,
            ver_logged: false,
            res_ver: 0,
            pad_prev: 0,
            kb_wait: false,
            rep_at: None,
            frame: 0,
            font: String::new(),
            inits: 0,
            ran: false,
            init_fails: 0,
            font_step: 0,
        }
    }

    fn close(&mut self) {
        if let Some(api) = &self.api {
            api.close();
        }
        self.open = false;
        self.release_wait = true;
        self.closed_at = Instant::now();
    }

    /// 手柄：按下的那一下（十字键 / 摇杆按住会连发）
    fn pad_pressed(&mut self) -> u16 {
        let now = pad_buttons();
        let pressed = now & !self.pad_prev;
        let dirs = P_UP | P_DOWN | P_LEFT | P_RIGHT;
        let held = now & dirs;
        let mut rep = 0;
        if held != 0 && held == self.pad_prev & dirs {
            match self.rep_at {
                Some(t) if t.elapsed() >= REPEAT_EVERY => {
                    rep = held;
                    self.rep_at = Some(Instant::now());
                }
                None => self.rep_at = Some(Instant::now() + REPEAT_DELAY - REPEAT_EVERY),
                _ => {}
            }
        } else {
            self.rep_at = None;
        }
        self.pad_prev = now;
        pressed | rep
    }

    /// 读快照 / 结果（几帧一次）
    fn poll(&mut self) {
        let Some(api) = self.api else { return };
        let (v, text) = api.get(1);
        if v != self.res_ver {
            self.res_ver = v;
            let ok = text.starts_with("ok\t");
            // 装备工坊（继承 / 重铸 / 超限）成功：放一声铁砧（sound.rs，dll\dark_ui\anvil.wav）；魇印、咒痕不放
            if ok && self.ws.mode == 0 {
                let wav = root().join("dll").join("dark_ui").join("anvil.wav");
                if !sound::play(&wav) {
                    dlog(&format!("铁砧声没放出来（{} 没有，或者 winmm 载不进来）", wav.display()));
                }
            }
            self.ws.done(ok, text.split_once('\t').map_or(text.clone(), |x| x.1.to_string()));
        }
        let (v, text) = api.get(0);
        if v != self.snap_ver && v != 0 {
            self.snap_ver = v;
            self.ws.set_snap(Snap::parse(&text));
            self.ws.clamp();
        }
        // 明细排在快照后面取：换快照时 dark_core 清掉了旧的，这里拿到的都是这份快照的
        let (_, text) = api.get(3);
        if !text.is_empty() {
            self.ws.merge_patch(&text);
        }
        let (v, text) = api.get(2);
        if v != self.prev_ver && v != 0 {
            self.prev_ver = v;
            self.ws.preview = Prev::parse(&text);
        }
    }
}

impl ImguiRenderLoop for App {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _render_context: &'a mut dyn RenderContext) {
        // hudhook 重建画面管线（交换链换了、格式或缓冲数变了，或者上次字体图集建不出来）时会再调一次，每次给新的 ImGui 上下文和新的贴图表：
        // 字体每次装进新上下文（文件只读一次），旧贴图编号作废；输入钩子、解码线程、素材、词条百科只做第一次。
        // 字体图集建不出来时 hudhook 每帧都重建：连着没建成就少装些字（fonts::add_at，退了就不再回去），日志只记前 4 次、第 8、16、32… 次和退字体那次
        self.inits += 1;
        let again = self.inits > 1;
        self.init_fails = if again && !self.ran { self.init_fails + 1 } else { 0 };
        self.ran = false;
        let step = self.font_step.max(self.init_fails.min(2));
        let log = !again || self.inits <= 4 || self.inits.is_power_of_two() || step != self.font_step;
        if !again {
            dlog("初始化开始");
        } else if log {
            let fail = if self.init_fails > 0 { format!("，上次没建成（连着 {} 次）", self.init_fails) } else { String::new() };
            dlog(&format!("再次初始化（第 {} 次）：hudhook 重建了画面管线{fail}；贴图重新传，输入钩子不重装", self.inits));
        }
        DRAWN.store(true, Ordering::Relaxed);
        self.tex.reset();
        let r = root();
        // 字体只装用得到的字：词条库、咒痕的名字和说明（curses.txt）、骨灰词条（ashes.txt 只要 ashcx 行，别的行是 dark_core 用的骨灰名字说明，不装）、
        // 源码里的文字（fonts.rs 的 SOURCES）。再次初始化时不用读（字形范围 fonts.rs 第一次就算好了，词条百科也还在）
        let mut lib = String::new();
        if !again {
            let read = |f: &str| std::fs::read_to_string(r.join("dll").join("dark_core").join(f)).unwrap_or_default();
            lib = ["affixes.txt", "curses.txt"].iter().map(|f| read(f) + "\n").collect::<String>();
            lib.extend(read("ashes.txt").lines().filter(|x| x.starts_with("ashcx\t")).map(|x| format!("{x}\n")));
        }
        let (font, ids) = fonts::add_at(ctx, &lib, &r.join("out").join("menu"), step);
        if !again {
            let frames = std::fs::read_to_string(r.join("launcher").join("reroll").join("data").join("menu").join("dr_frames.txt")).unwrap_or_default();
            self.ws.codex = ui::Codex::parse(&lib, &frames);
        }
        if log {
            dlog(&format!("字体：{font}（中文 {}、英文 {}）", ids[0].is_some(), ids[1].is_some()));
        }
        self.font = font;
        self.font_step = step;
        self.ws.fonts = ids.map(|f| f.map(ui::UiFont));
        ctx.set_ini_filename(None::<PathBuf>);
        self.hooks = hooks::install();
        if again {
            return;
        }
        dlog(&format!("输入钩子：光标 {}、键鼠 {}、手柄 {}", self.hooks.0, self.hooks.1, self.hooks.2));
        let a = Assets::load(&r.join("out").join("menu").join("dark_ui_sprites.bin"), &r.join("launcher").join("reroll").join("data").join("menu"));
        dlog(&format!("素材：{}", a.note));
        if !a.has_sprites() {
            self.ws.note = "没有原版界面素材（out\\menu\\dark_ui_sprites.bin，启动器合成菜单包时生成）：界面是简化的样子".into();
        }
        let a = Arc::new(a);
        self.tex.start(a.clone());
        self.assets = Some(a);
        dlog("初始化完成");
    }

    fn before_render<'a>(&'a mut self, ctx: &mut imgui::Context, render_context: &'a mut dyn RenderContext) {
        self.ran = true;
        ctx.io_mut().mouse_draw_cursor = self.open;
        hooks::BLOCK.store(self.open || self.release_wait, Ordering::Relaxed);
        hooks::set_typing(self.open && self.ws.typing());
        if self.open {
            hooks::free_cursor();
        }
        if self.open {
            if let Some(p) = hooks::cursor_client() {
                ctx.io_mut().add_mouse_pos_event(p);
            }
        }
        self.tex.pump(render_context);
    }

    fn render(&mut self, ui: &mut Ui) {
        self.frame += 1;
        if self.frame == 1 {
            dlog(&format!("第一帧，屏幕 {:?}", ui.io().display_size));
        }
        // 刚关窗口：手柄、键、鼠标都松开了才放行游戏输入（最多等 RELEASE_MAX，防止漏了松开的消息一直挡着）
        if self.release_wait && !self.open {
            use hudhook::imgui::{Key, MouseButton};
            let keys = [Key::Escape, Key::Backspace, Key::Enter, Key::KeypadEnter, Key::Space, Key::Tab, Key::UpArrow, Key::DownArrow, Key::LeftArrow, Key::RightArrow];
            let held = pad_buttons() != 0 || keys.iter().any(|&k| ui.is_key_down(k)) || [MouseButton::Left, MouseButton::Right].iter().any(|&b| ui.is_mouse_down(b));
            if !held || self.closed_at.elapsed() > RELEASE_MAX {
                self.release_wait = false;
            }
        }
        if self.api.is_none() && self.api_try.elapsed() > Duration::from_secs(1) {
            self.api_try = Instant::now();
            self.api = Api::find();
            self.api_tries += 1;
            match (&self.api, self.api_tries) {
                (Some(_), _) => dlog(&format!("找到 dark_core 的接口（第 {} 次找）", self.api_tries)),
                (None, 1 | 10 | 60) => dlog(&format!("还没找到 dark_core 的接口（第 {} 次）：{}", self.api_tries, data::Api::missing())),
                _ => {}
            }
        }
        let Some(api) = self.api else { return };
        let (ver, want, mode) = api.state();
        if ver != API {
            if !self.ver_logged {
                self.ver_logged = true;
                dlog(&format!("dark_core 的接口版本 {ver}，界面要 {API}：两个 DLL 不是同一版，不开窗口"));
            }
            if want && !self.open {
                api.close();
            }
            return;
        }
        if want && !self.open {
            dlog("开窗口");
            self.open = true;
            self.ws.reset();
            self.ws.set_mode(mode);
            self.ws.snap = None;
            self.pad_prev = pad_buttons(); // 打开时还按着的（选菜单的 A）不算
            self.kb_wait = ui::KB_KEYS.iter().any(|&k| ui.is_key_down(k)); // 键盘同样：松开以前不算
            self.res_ver = api.get(1).0;
            self.snap_ver = 0;
            self.prev_ver = api.get(2).0;
            for s in PRELOAD {
                self.tex.boot.push(TKey::Sprite(s.to_string()));
            }
            api.refresh();
        } else if !want && self.open {
            dlog("关窗口（dark_core 那边关了）");
            self.open = false;
            self.release_wait = true;
            self.closed_at = Instant::now();
        }
        if !self.open {
            return;
        }
        if self.frame % 6 == 0 || self.ws.waiting || self.ws.snap.is_none() || self.ws.loading() || (self.ws.page == 1 && self.ws.preview.is_none()) {
            self.poll();
        }
        let pressed = self.pad_pressed();
        if self.kb_wait && !ui::KB_KEYS.iter().any(|&k| ui.is_key_down(k)) {
            self.kb_wait = false;
        }
        let act = Workshop::keys(ui, pressed, self.ws.typing() || self.kb_wait);
        let screen = ui.io().display_size;
        self.tex.begin_frame();
        self.ws.frame(ui, &mut self.tex, act, screen);
        for r in std::mem::take(&mut self.ws.reqs) {
            match r {
                ui::Req::Close => self.close(),
                ui::Req::Apply { src, k, target, replace } => api.apply(src, k, target, replace),
                ui::Req::Reforge(k) => api.reforge(k),
                ui::Req::Preview(k) => api.preview(k),
                ui::Req::Overlimit { cat, k, n } => api.overlimit(cat, k, n),
                ui::Req::Affix { k, i } => api.affix(k, i),
                ui::Req::Discard(k) => api.discard(k),
                ui::Req::Seal { slot, fam } => api.seal(slot, fam),
                ui::Req::Want(t) => api.want(&t),
            }
        }
        if self.hooks.1 < 2 || !self.hooks.2 {
            self.ws.note = format!("输入钩子：键鼠 {} 个、手柄 {}，游戏可能还会收到输入", self.hooks.1, if self.hooks.2 { "挂上了" } else { "没挂上" });
        }
    }

    fn before_wnd_proc(&self, hwnd: hudhook::windows::Win32::Foundation::HWND, _umsg: u32, _w: hudhook::windows::Win32::Foundation::WPARAM, _l: hudhook::windows::Win32::Foundation::LPARAM) -> hudhook::BeforeWndProc {
        hooks::set_window(hwnd);
        hudhook::BeforeWndProc::Continue
    }

    fn message_filter(&self, _io: &imgui::Io) -> MessageFilter {
        if self.open || self.release_wait { MessageFilter::InputAll } else { MessageFilter::empty() }
    }
}

/// 入口：和 hudhook::hudhook! 宏做的一样（开线程挂 DirectX 12 的钩子，挂不上就 eject），但明确返回 TRUE（同 dll\dark_dev：
/// hudhook 0.9.3 的宏生成的 DllMain 没有返回值，偶尔被当成加载失败卸掉，游戏崩在 *_unloaded）
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(hmodule: hudhook::windows::Win32::Foundation::HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == hudhook::windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH {
        let raw = hmodule.0 as usize;
        let _ = HMODULE.set(raw);
        std::thread::spawn(move || {
            let t = local_time();
            dlog(&format!(
                "加载（{}，{}-{:02}-{:02} {:02}:{:02}:{:02}，进程 {}）",
                env!("CARGO_PKG_VERSION"),
                t.year,
                t.month,
                t.day,
                t.hour,
                t.min,
                t.sec,
                std::process::id()
            ));
            std::panic::set_hook(Box::new(|p| dlog(&format!("panic：{p}"))));
            let _ = hudhook::tracing::subscriber::set_global_default(HudLog);
            let hmodule = hudhook::windows::Win32::Foundation::HINSTANCE(raw as _);
            // 挂不上隔 2 秒再试（最多 5 次）：dll\dark_dev 等别的悬浮层也在游戏刚启动时挂同样的钩子，偶尔撞上就失败
            // （2026-10-31 一局赐福提示「没有加载界面」，怀疑就是这样被卸掉的）；和 dark_dev 用 HookLock 排队
            for n in 1..=5 {
                let lock = HookLock::take();
                if lock.is_none() {
                    dlog("等别的悬浮层挂钩子等了 30 秒（或者建不了互斥量），不等了");
                }
                let r = hudhook::Hudhook::builder().with::<ImguiDx12Hooks>(App::new()).with_hmodule(hmodule).build().apply();
                drop(lock);
                match r {
                    Ok(()) => {
                        dlog(&format!("DirectX 12 钩子挂上了（第 {n} 次）"));
                        // 挂上了却一直没画过：钩子被别的悬浮层盖掉了（或者 hudhook 初始化不了，看上面的 hudhook 行）
                        for s in 1..=120 {
                            std::thread::sleep(Duration::from_secs(1));
                            if DRAWN.load(Ordering::Relaxed) {
                                return;
                            }
                            if s == 30 || s == 120 {
                                dlog(&format!("钩子挂上 {s} 秒了还没画过一帧：可能被别的悬浮层盖掉了，赐福的装备工坊会提示没有加载界面（重启游戏一般就好）"));
                            }
                        }
                        return;
                    }
                    Err(e) => {
                        dlog(&format!("DirectX 12 钩子第 {n} 次挂不上：{e:?}"));
                        hudhook::tracing::error!("Couldn't apply hooks: {e:?}");
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            dlog("挂不上，放弃（赐福的装备工坊会提示没有加载界面）");
            hudhook::eject();
        });
    }
    BOOL(1)
}
