//! Simple random tile-based level generator (no external dependencies).
//!
//! Build & run:
//!   rustc -O levelgen.rs && ./levelgen
//!   ./levelgen --rooms 10 --min-width 40 --max-width 60 \
//!              --min-height 20 --max-height 30 --min-room 4 --max-room 9 --seed 42
//!
//! Add `--ints` to print the raw integer tile IDs instead of ASCII.

use std::env;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Tile identifiers
// ---------------------------------------------------------------------------
pub type Tile = u8;

pub const TILE_VOID: Tile = 0;
pub const TILE_FLOOR: Tile = 1;
pub const TILE_WALL: Tile = 2;

// ---------------------------------------------------------------------------
// Tiny RNG (xorshift64*) so we don't need the `rand` crate
// ---------------------------------------------------------------------------
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // State must be non-zero.
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Random integer in [lo, hi] (inclusive).
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % ((hi - lo + 1) as u64)) as usize
    }

    fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 0
    }
}

// ---------------------------------------------------------------------------
// Config / Map / Room
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct Config {
    pub num_rooms: usize,
    pub min_width: usize,
    pub max_width: usize,
    pub min_height: usize,
    pub max_height: usize,
    pub min_room_size: usize,
    pub max_room_size: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy)]
struct Room {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Room {
    fn center(&self) -> (usize, usize) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// True if the rooms overlap or are closer than `gap` tiles apart.
    fn collides(&self, other: &Room, gap: usize) -> bool {
        self.x < other.x + other.w + gap
            && self.x + self.w + gap > other.x
            && self.y < other.y + other.h + gap
            && self.y + self.h + gap > other.y
    }
}

/// Row-major index of tile (x, y) in a flat array of the given width.
#[inline]
pub fn idx(x: usize, y: usize, width: usize) -> usize {
    y * width + x
}

pub struct Map {
    pub width: usize,
    pub height: usize,
    /// Flat, row-major tile array of length `width * height`.
    /// Tile (x, y) lives at `tiles[y * width + x]`.
    pub tiles: Vec<Tile>,
}

impl Map {
    /// Get the tile at (x, y). Panics if out of bounds.
    pub fn get(&self, x: usize, y: usize) -> Tile {
        assert!(x < self.width && y < self.height, "tile out of bounds");
        self.tiles[idx(x, y, self.width)]
    }

    pub fn print_ascii(&self) {
        for row in self.tiles.chunks(self.width) {
            let line: String = row
                .iter()
                .map(|&t| match t {
                    TILE_FLOOR => '.',
                    TILE_WALL => '#',
                    _ => ' ',
                })
                .collect();
            println!("{}", line);
        }
    }

    pub fn print_ints(&self) {
        for row in self.tiles.chunks(self.width) {
            let line: Vec<String> = row.iter().map(|t| t.to_string()).collect();
            println!("{}", line.join(" "));
        }
    }
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------
pub fn generate(cfg: &Config) -> Result<Map, String> {
    validate(cfg)?;
    let mut rng = Rng::new(cfg.seed);

    let width = rng.range(cfg.min_width, cfg.max_width);
    let height = rng.range(cfg.min_height, cfg.max_height);
    let mut tiles: Vec<Tile> = vec![TILE_VOID; width * height];

    // 1. Place rooms (keep a 1-tile border free so outer walls fit in the map).
    let mut rooms: Vec<Room> = Vec::new();
    let max_attempts = cfg.num_rooms * 50;
    let mut attempts = 0;

    while rooms.len() < cfg.num_rooms && attempts < max_attempts {
        attempts += 1;

        let w = rng.range(cfg.min_room_size, cfg.max_room_size);
        let h = rng.range(cfg.min_room_size, cfg.max_room_size);
        if w + 2 > width || h + 2 > height {
            continue;
        }

        let x = rng.range(1, width - w - 1);
        let y = rng.range(1, height - h - 1);
        let room = Room { x, y, w, h };

        if rooms.iter().any(|r| room.collides(r, 1)) {
            continue;
        }

        carve_room(&mut tiles, width, &room);
        rooms.push(room);
    }

    if rooms.len() < cfg.num_rooms {
        eprintln!(
            "warning: only placed {} of {} rooms (map too small / crowded)",
            rooms.len(),
            cfg.num_rooms
        );
    }

    // 2. Connect consecutive rooms with L-shaped corridors.
    for pair in rooms.windows(2) {
        let (x1, y1) = pair[0].center();
        let (x2, y2) = pair[1].center();
        if rng.coin() {
            carve_h_corridor(&mut tiles, width, x1, x2, y1);
            carve_v_corridor(&mut tiles, width, y1, y2, x2);
        } else {
            carve_v_corridor(&mut tiles, width, y1, y2, x1);
            carve_h_corridor(&mut tiles, width, x1, x2, y2);
        }
    }

    // 3. Add walls around every floor tile (including diagonals).
    add_walls(&mut tiles, width, height);

    Ok(Map { width, height, tiles })
}

fn validate(cfg: &Config) -> Result<(), String> {
    if cfg.num_rooms == 0 {
        return Err("rooms must be at least 1".into());
    }
    if cfg.min_width > cfg.max_width {
        return Err("min-width must be <= max-width".into());
    }
    if cfg.min_height > cfg.max_height {
        return Err("min-height must be <= max-height".into());
    }
    if cfg.min_room_size == 0 || cfg.min_room_size > cfg.max_room_size {
        return Err("room size must satisfy 1 <= min-room <= max-room".into());
    }
    // Room + 1 tile of wall on each side must fit in the smallest possible map.
    if cfg.max_room_size + 2 > cfg.min_width || cfg.max_room_size + 2 > cfg.min_height {
        return Err(format!(
            "max-room ({}) + 2 for walls must fit within min-width ({}) and min-height ({})",
            cfg.max_room_size, cfg.min_width, cfg.min_height
        ));
    }
    Ok(())
}

fn carve_room(tiles: &mut [Tile], width: usize, r: &Room) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            tiles[idx(x, y, width)] = TILE_FLOOR;
        }
    }
}

fn carve_h_corridor(tiles: &mut [Tile], width: usize, x1: usize, x2: usize, y: usize) {
    for x in x1.min(x2)..=x1.max(x2) {
        tiles[idx(x, y, width)] = TILE_FLOOR;
    }
}

fn carve_v_corridor(tiles: &mut [Tile], width: usize, y1: usize, y2: usize, x: usize) {
    for y in y1.min(y2)..=y1.max(y2) {
        tiles[idx(x, y, width)] = TILE_FLOOR;
    }
}

fn add_walls(tiles: &mut [Tile], width: usize, height: usize) {
    // Work from a snapshot so newly placed walls don't spawn more walls.
    let snapshot: Vec<Tile> = tiles.to_vec();

    for y in 0..height {
        for x in 0..width {
            if snapshot[idx(x, y, width)] != TILE_VOID {
                continue;
            }
            'neighbors: for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    if snapshot[idx(nx as usize, ny as usize, width)] == TILE_FLOOR {
                        tiles[idx(x, y, width)] = TILE_WALL;
                        break 'neighbors;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------
fn parse_args() -> Result<(Config, bool), String> {
    let default_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345);

    let mut cfg = Config {
        num_rooms: 8,
        min_width: 40,
        max_width: 60,
        min_height: 20,
        max_height: 30,
        min_room_size: 4,
        max_room_size: 10,
        seed: default_seed,
    };
    let mut print_ints = false;

    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag == "--ints" {
            print_ints = true;
            i += 1;
            continue;
        }
        if flag == "--help" || flag == "-h" {
            print_help();
            process::exit(0);
        }

        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("missing value for {}", flag))?;
        let num: u64 = value
            .parse()
            .map_err(|_| format!("invalid number for {}: {}", flag, value))?;

        match flag {
            "--rooms" => cfg.num_rooms = num as usize,
            "--min-width" => cfg.min_width = num as usize,
            "--max-width" => cfg.max_width = num as usize,
            "--min-height" => cfg.min_height = num as usize,
            "--max-height" => cfg.max_height = num as usize,
            "--min-room" => cfg.min_room_size = num as usize,
            "--max-room" => cfg.max_room_size = num as usize,
            "--seed" => cfg.seed = num,
            _ => return Err(format!("unknown option: {}", flag)),
        }
        i += 2;
    }
    Ok((cfg, print_ints))
}

fn print_help() {
    println!(
        "Usage: levelgen [OPTIONS]

  --rooms N        number of rooms            (default 8)
  --min-width N    minimum map width          (default 40)
  --max-width N    maximum map width          (default 60)
  --min-height N   minimum map height         (default 20)
  --max-height N   maximum map height         (default 30)
  --min-room N     minimum room side length   (default 4)
  --max-room N     maximum room side length   (default 10)
  --seed N         RNG seed (random if omitted)
  --ints           print integer tile IDs instead of ASCII

Tile IDs: {} = void, {} = floor, {} = wall",
        TILE_VOID, TILE_FLOOR, TILE_WALL
    );
}

fn main() {
    let (cfg, print_ints) = parse_args().unwrap_or_else(|e| {
        eprintln!("error: {}\n(try --help)", e);
        process::exit(1);
    });

    let map = generate(&cfg).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("Map {}x{}  (seed {})", map.width, map.height, cfg.seed);
    if print_ints {
        map.print_ints();
    } else {
        map.print_ascii();
    }
}
