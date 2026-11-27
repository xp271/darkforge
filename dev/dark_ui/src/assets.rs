//! 界面用的图（都是 BC7 等压缩块，用到时解成 RGBA）：
//!   原版菜单素材：out\menu\dark_ui_sprites.bin（启动器 menu.py 合成菜单包时从本机 Data0 的原版图集裁出来，不打包；格式见 menu.py）
//!   物品图标：launcher\reroll\data\menu\dr_frames_NN.dds / .layout（稀有度边框图标和自制道具图标，make_frames.py 画的，打包带）
//! 快照里的图标编号是游戏参数里现在的（生成装备都是带框的那份），原版编号（菜单包没合成成功时）这里没有，界面画个空格子
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 解好的图
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

struct SpriteSrc {
    fmt: u32,
    bw: u32,
    bh: u32,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
    off: usize,
    len: usize,
}

pub struct Assets {
    sprites: HashMap<String, SpriteSrc>,
    blob: Vec<u8>,
    /// 图标编号 → (图集序号, x, y)，图标都是 160×160
    icons: HashMap<u16, (usize, u32, u32)>,
    atlases: Vec<PathBuf>,
    /// 读到了什么（给日志 / 界面角落的提示）
    pub note: String,
}

pub const ICON: u32 = 160;

impl Assets {
    /// sprites：dark_ui_sprites.bin；frames：放 dr_frames_NN 的目录。缺了的部分就空着（界面照样能用）
    pub fn load(sprites: &Path, frames: &Path) -> Assets {
        let mut a = Assets { sprites: HashMap::new(), blob: Vec::new(), icons: HashMap::new(), atlases: Vec::new(), note: String::new() };
        if let Ok(raw) = fs::read(sprites) {
            if let Some(cut) = raw.windows(2).position(|w| w == b"\n\n") {
                let head = String::from_utf8_lossy(&raw[..cut]).into_owned();
                a.blob = raw[cut + 2..].to_vec();
                for l in head.lines().skip(1) {
                    let f: Vec<&str> = l.split('\t').collect();
                    if f.len() < 10 {
                        continue;
                    }
                    let n = |i: usize| f[i].parse::<usize>().unwrap_or(0);
                    let s = SpriteSrc {
                        fmt: n(1) as u32,
                        bw: n(2) as u32,
                        bh: n(3) as u32,
                        dx: n(4) as u32,
                        dy: n(5) as u32,
                        w: n(6) as u32,
                        h: n(7) as u32,
                        off: n(8),
                        len: n(9),
                    };
                    if s.off + s.len <= a.blob.len() {
                        a.sprites.insert(f[0].to_string(), s);
                    }
                }
            }
        }
        for k in 0.. {
            let dds = frames.join(format!("dr_frames_{k:02}.dds"));
            let Ok(lay) = fs::read_to_string(frames.join(format!("dr_frames_{k:02}.layout"))) else { break };
            if !dds.is_file() {
                break;
            }
            for l in lay.lines() {
                let num = |key: &str| -> Option<u32> {
                    let i = l.find(&format!("{key}=\""))? + key.len() + 2;
                    l[i..].split('"').next()?.parse().ok()
                };
                let id = l.find("MENU_ItemIcon_").and_then(|i| l[i + 14..].split('.').next()?.parse::<u16>().ok());
                if let (Some(id), Some(x), Some(y)) = (id, num("x"), num("y")) {
                    a.icons.insert(id, (a.atlases.len(), x, y));
                }
            }
            a.atlases.push(dds);
        }
        a.note = format!("素材 {} 个、图标 {} 个（{} 张图集）", a.sprites.len(), a.icons.len(), a.atlases.len());
        a
    }

    pub fn has_sprites(&self) -> bool {
        !self.sprites.is_empty()
    }

    /// 原版素材（解出来裁成精灵本身的大小）；DR_Lock = 锁定图标（和 dark_lock 在背包格子里画的同一张），
    /// DR_Discard = 「弃置」的红叉（用户 2026-11-26 画的，42×42）
    pub fn sprite(&self, name: &str) -> Option<Rgba> {
        if name == "DR_Lock" {
            return lock_icon();
        }
        if name == "DR_Discard" {
            return discard_icon();
        }
        let s = self.sprites.get(name)?;
        let full = decode(s.fmt, &self.blob[s.off..s.off + s.len], s.bw, s.bh)?;
        let mut data = vec![0u8; (s.w * s.h * 4) as usize];
        for y in 0..s.h.min(s.bh - s.dy) {
            let src = (((y + s.dy) * s.bw + s.dx) * 4) as usize;
            let n = (s.w.min(s.bw - s.dx) * 4) as usize;
            data[(y * s.w * 4) as usize..(y * s.w * 4) as usize + n].copy_from_slice(&full[src..src + n]);
        }
        Some(Rgba { w: s.w, h: s.h, data })
    }

    /// 物品图标（160×160）：从图集文件里读那 40 行块
    pub fn icon(&self, id: u16) -> Option<Rgba> {
        let &(ai, x, y) = self.icons.get(&id)?;
        let mut f = File::open(&self.atlases[ai]).ok()?;
        let mut head = [0u8; 148];
        f.read_exact(&mut head).ok()?;
        let aw = u32::from_le_bytes(head[16..20].try_into().ok()?);
        let fmt = if &head[84..88] == b"DX10" { u32::from_le_bytes(head[128..132].try_into().ok()?) } else { return None };
        let bsz = block_size(fmt)?;
        let row_bytes = (aw / 4 * bsz) as u64;
        let n = (ICON / 4 * bsz) as usize;
        let mut blocks = vec![0u8; n * (ICON / 4) as usize];
        // 一次读完这 40 行（以前一行一次定位，机械硬盘上一个图标要 40 次寻道）
        let rb = row_bytes as usize;
        let mut rows = vec![0u8; rb * (ICON / 4) as usize];
        f.seek(SeekFrom::Start(148 + (y / 4) as u64 * row_bytes)).ok()?;
        f.read_exact(&mut rows).ok()?;
        let x0 = (x / 4 * bsz) as usize;
        for (by, b) in blocks.chunks_exact_mut(n).enumerate() {
            b.copy_from_slice(&rows[by * rb + x0..by * rb + x0 + n]);
        }
        Some(Rgba { w: ICON, h: ICON, data: decode(fmt, &blocks, ICON, ICON)? })
    }
}

/// dark_lock 的锁图（dev\dark_lock\src\lock_bitmap.bin：40×42，和原版「装备中」标记 MENU_EquMark 同画风）
fn lock_icon() -> Option<Rgba> {
    const LOCK: &[u8] = include_bytes!("../../dark_lock/src/lock_bitmap.bin");
    swf_bitmap(LOCK)
}

/// 「弃置」的红叉（dev\dark_ui\src\discard_bitmap.bin：42×42，用户 2026-11-26 自己画的；容器格式和锁图一样）
fn discard_icon() -> Option<Rgba> {
    const ICON: &[u8] = include_bytes!("discard_bitmap.bin");
    swf_bitmap(ICON)
}

/// SWF DefineBitsLossless2 的正文（格式 5 + 宽 u16 + 高 u16 + zlib(预乘 alpha 的 ARGB)）→ RGBA
fn swf_bitmap(b: &[u8]) -> Option<Rgba> {
    if b.len() < 5 || b[0] != 5 {
        return None;
    }
    let (w, h) = (u16::from_le_bytes([b[1], b[2]]) as u32, u16::from_le_bytes([b[3], b[4]]) as u32);
    let argb = miniz_oxide::inflate::decompress_to_vec_zlib(&b[5..]).ok()?;
    if argb.len() < (w * h * 4) as usize {
        return None;
    }
    let data = argb
        .chunks_exact(4)
        .take((w * h) as usize)
        .flat_map(|p| {
            let a = p[0] as u32;
            let un = |c: u8| if a > 0 { (c as u32 * 255 / a).min(255) as u8 } else { 0 };
            [un(p[1]), un(p[2]), un(p[3]), p[0]]
        })
        .collect();
    Some(Rgba { w, h, data })
}

fn block_size(fmt: u32) -> Option<u32> {
    match fmt {
        71 | 72 => Some(8),
        74 | 75 | 77 | 78 | 98 | 99 => Some(16),
        _ => None,
    }
}

/// 一片块（宽 w、高 h 像素，4 对齐，按行排）解成 RGBA
fn decode(fmt: u32, src: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let bsz = block_size(fmt)? as usize;
    let (bw, bh) = ((w / 4) as usize, (h / 4) as usize);
    if src.len() < bw * bh * bsz {
        return None;
    }
    let pitch = w as usize * 4;
    let mut out = vec![0u8; pitch * h as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let b = &src[(by * bw + bx) * bsz..(by * bw + bx + 1) * bsz];
            let dst = &mut out[by * 4 * pitch + bx * 16..];
            match fmt {
                71 | 72 => bcdec_rs::bc1(b, dst, pitch),
                74 | 75 => bcdec_rs::bc2(b, dst, pitch),
                77 | 78 => bcdec_rs::bc3(b, dst, pitch),
                _ => bcdec_rs::bc7(b, dst, pitch),
            }
        }
    }
    Some(out)
}
