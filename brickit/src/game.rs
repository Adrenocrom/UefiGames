//! Game state and physics for BrickShot.

use crate::draw::{self, Screen};
use crate::math;

const PADDLE_W: f32 = 110.0;
const PADDLE_H: f32 = 14.0;
const PADDLE_Y_OFFSET: f32 = 40.0; // distance of the paddle above the bottom edge
// Bounds and step for the grow/shrink collectibles.
const PADDLE_MIN: f32 = 60.0;
const PADDLE_MAX: f32 = 220.0;
const PADDLE_RESIZE: f32 = 30.0;

const BALL_R: f32 = 6.0;
const BALL_SPEED: f32 = 7.0; // px per frame
const MAX_BALLS: usize = 8;

// Collectibles dropped by destroyed bricks.
const MAX_DROPS: usize = 100;
const DROP_SIZE: f32 = 20.0;
const DROP_SPEED: f32 = 2.5; // px per frame
const DROP_CHANCE: u32 = 75; // percent chance per destroyed brick

// Main menu layout: centered bars, the last of which quits the game.
const MENU_W: f32 = 340.0;
const MENU_ITEM_H: f32 = 26.0;
const MENU_GAP: f32 = 10.0;

const COLS: usize = 12;
const ROWS: usize = 9;
const BRICK_W: f32 = 64.0;
const BRICK_H: f32 = 22.0;
const BRICK_GAP: f32 = 6.0;
const BRICK_TOP: f32 = 60.0;

const COL_BG: draw::Color = draw::Color::new(12, 12, 20);
const COL_WALL: draw::Color = draw::Color::new(70, 70, 90);
const COL_PADDLE: draw::Color = draw::Color::new(230, 230, 240);
const COL_BALL: draw::Color = draw::Color::new(255, 255, 255);
const COL_TEXT: draw::Color = draw::Color::new(200, 200, 210);
const COL_DIM: draw::Color = draw::Color::new(110, 110, 125);
const COL_MENU_ITEM: draw::Color = draw::Color::new(34, 34, 52);
const COL_MENU_SEL: draw::Color = draw::Color::new(60, 140, 230);
const COL_DROP_GROW: draw::Color = draw::Color::new(80, 200, 90);
const COL_DROP_SHRINK: draw::Color = draw::Color::new(230, 60, 60);
const COL_DROP_MULTI: draw::Color = draw::Color::new(60, 180, 230);

const ROW_COLORS: [draw::Color; ROWS] = [
    draw::Color::new(230, 60, 60),
    draw::Color::new(240, 140, 40),
    draw::Color::new(240, 210, 50),
    draw::Color::new(80, 200, 90),
    draw::Color::new(60, 140, 230),
    draw::Color::new(80, 200, 90),
    draw::Color::new(240, 210, 50),
    draw::Color::new(240, 140, 40),
    draw::Color::new(230, 60, 60),
];

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Menu,
    Ready,
    Playing,
    LevelClear,
    GameOver,
}

/// Difficulty presets selectable in the main menu.
#[derive(Clone, Copy)]
enum Difficulty {
    Simple,
    Normal,
    Hard,
}

impl Difficulty {
    fn lives(self) -> u32 {
        match self {
            Difficulty::Simple => 10,
            Difficulty::Normal => 5,
            Difficulty::Hard => 3,
        }
    }

    /// Base paddle width; grow/shrink collectibles modify it from there.
    fn paddle_w(self) -> f32 {
        match self {
            Difficulty::Simple => 150.0,
            Difficulty::Normal => PADDLE_W,
            Difficulty::Hard => 80.0,
        }
    }
}

/// Main menu entries: label plus the difficulty they start (None = quit).
const MENU_ITEMS: [(&str, Option<Difficulty>); 4] = [
    ("SIMPLE - 10 LIVES - BIG BAR", Some(Difficulty::Simple)),
    ("NORMAL - 5 LIVES", Some(Difficulty::Normal)),
    ("HARD - 3 LIVES - SMALL BAR", Some(Difficulty::Hard)),
    ("QUIT", None),
];

/// What a collectible does when caught with the paddle.
#[derive(Clone, Copy)]
enum DropKind {
    Grow,
    Shrink,
    MultiBall,
}

/// A collectible falling from a destroyed brick.
#[derive(Clone, Copy)]
struct Drop {
    x: f32,
    y: f32,
    kind: DropKind,
}

#[derive(Clone, Copy)]
struct Ball {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
}

/// Events produced by advancing one ball one frame.
struct BallStep {
    /// The ball fell past the bottom edge.
    lost: bool,
    /// Brick destroyed by this ball (at most one per frame).
    brick: Option<usize>,
}

pub struct Game {
    width: f32,
    height: f32,
    paddle_x: f32,
    paddle_w: f32,
    balls: [Option<Ball>; MAX_BALLS],
    drops: [Option<Drop>; MAX_DROPS],
    bricks: [bool; COLS * ROWS],
    bricks_left: usize,
    lives: u32,
    score: u32,
    phase: Phase,
    /// Virtual cursor position (the menu needs both axes, gameplay only x).
    cursor_x: f32,
    cursor_y: f32,
    /// Highlighted main menu entry.
    menu_sel: usize,
    /// Set when the player picked QUIT or pressed ESC in the menu.
    quit: bool,
    difficulty: Difficulty,
    /// Xorshift32 state, seeded from the resolution (UEFI offers no entropy
    /// source). Only rolls collectible spawns, so quality is irrelevant.
    rng: u32,
}

impl Game {
    pub fn new(width: f32, height: f32) -> Game {
        let mut g = Game {
            width,
            height,
            paddle_x: width / 2.0,
            paddle_w: PADDLE_W,
            balls: [None; MAX_BALLS],
            drops: [None; MAX_DROPS],
            bricks: [true; COLS * ROWS],
            bricks_left: COLS * ROWS,
            lives: 5, // placeholder; start_game() applies the difficulty
            score: 0,
            phase: Phase::Menu,
            cursor_x: width / 2.0,
            cursor_y: height / 2.0,
            menu_sel: 1, // NORMAL
            quit: false,
            difficulty: Difficulty::Normal,
            rng: (0x9E37_79B9 ^ (width as u32) ^ ((height as u32) << 16)) | 1,
        };
        g.reset_balls();
        g
    }

    /// One ball resting on the paddle, all others removed.
    fn reset_balls(&mut self) {
        self.balls = [None; MAX_BALLS];
        self.balls[0] = Some(Ball {
            x: self.paddle_x,
            y: self.paddle_y() - BALL_R - 2.0,
            dx: 0.0,
            dy: 0.0,
        });
    }

    fn paddle_y(&self) -> f32 {
        self.height - PADDLE_Y_OFFSET - PADDLE_H
    }

    fn grid_left(&self) -> f32 {
        (self.width - (COLS as f32 * (BRICK_W + BRICK_GAP) - BRICK_GAP)) / 2.0
    }

    /// Mouse click or space bar: launch the ball, start the next round, or
    /// restart after game over.
    pub fn primary_action(&mut self) {
        match self.phase {
            Phase::Menu => {
                if let Some((_, Some(diff))) = MENU_ITEMS.get(self.menu_sel) {
                    self.start_game(*diff);
                } else {
                    self.quit = true; // QUIT selected
                }
            }
            Phase::Ready => {
                self.phase = Phase::Playing;
                if let Some(ball) = self.balls[0].as_mut() {
                    ball.dx = BALL_SPEED * 0.6;
                    ball.dy = -BALL_SPEED;
                }
            }
            Phase::LevelClear => {
                // Next round: fresh brick wall, score and lives carry over.
                self.bricks = [true; COLS * ROWS];
                self.bricks_left = COLS * ROWS;
                self.drops = [None; MAX_DROPS];
                self.paddle_w = self.difficulty.paddle_w();
                self.phase = Phase::Ready;
                self.reset_balls();
            }
            Phase::GameOver => {
                // Back to the menu, keeping the last difficulty highlighted.
                self.phase = Phase::Menu;
            }
            Phase::Playing => {}
        }
    }

    /// Begin a fresh run with the chosen difficulty.
    fn start_game(&mut self, diff: Difficulty) {
        self.difficulty = diff;
        self.lives = diff.lives();
        self.paddle_w = diff.paddle_w();
        self.paddle_x = self.width / 2.0;
        self.score = 0;
        self.bricks = [true; COLS * ROWS];
        self.bricks_left = COLS * ROWS;
        self.drops = [None; MAX_DROPS];
        self.phase = Phase::Ready;
        self.reset_balls();
    }

    /// ESC: leave the current run for the menu, main menu ESC quits.
    pub fn escape(&mut self) {
        if self.phase == Phase::Menu {
            self.quit = true;
        } else {
            self.phase = Phase::Menu;
        }
    }

    /// Whether the player asked to leave the game.
    pub fn wants_quit(&self) -> bool {
        self.quit
    }

    /// Whether the main menu is on screen (LEFT/RIGHT mean nothing there).
    pub fn in_menu(&self) -> bool {
        self.phase == Phase::Menu
    }

    /// Arrow up in the menu: select the previous entry (no-op elsewhere).
    pub fn menu_up(&mut self) {
        if self.phase != Phase::Menu {
            return;
        }
        self.menu_sel = self.menu_sel.saturating_sub(1);
    }

    /// Arrow down in the menu: select the next entry (no-op elsewhere).
    pub fn menu_down(&mut self) {
        if self.phase != Phase::Menu {
            return;
        }
        self.menu_sel = (self.menu_sel + 1).min(MENU_ITEMS.len() - 1);
    }

    /// Advance one frame. `mouse_x`/`mouse_y` are the virtual cursor
    /// position (the menu needs both axes; gameplay uses only x).
    pub fn update(&mut self, mouse_x: f32, mouse_y: f32) {
        if self.phase == Phase::Menu {
            // Hover highlights an entry only while the cursor actually
            // moves. A resting cursor (the initial position is the screen
            // center, which sits on an entry) must not override the
            // arrow-key selection every frame.
            let moved =
                (mouse_x - self.cursor_x).abs() > 0.5 || (mouse_y - self.cursor_y).abs() > 0.5;
            self.cursor_x = mouse_x;
            self.cursor_y = mouse_y;
            if moved {
                if let Some(i) = self.menu_hit(mouse_x, mouse_y) {
                    self.menu_sel = i;
                }
            }
            return;
        }

        // The paddle tracks the mouse directly.
        self.paddle_x = mouse_x.clamp(self.paddle_w / 2.0, self.width - self.paddle_w / 2.0);

        if self.phase != Phase::Playing {
            if self.phase == Phase::Ready {
                // Keep the ball glued to the paddle while waiting for a click.
                self.reset_balls();
            }
            return;
        }

        let paddle_top = self.paddle_y();
        let grid_left = self.grid_left();

        // --- Balls ---
        for i in 0..MAX_BALLS {
            // The ball borrow ends with step_ball, so `self` is fully usable
            // again right after (spawning drops etc.).
            let step = {
                let Some(ball) = self.balls[i].as_mut() else { continue };
                step_ball(
                    ball,
                    self.width,
                    self.height,
                    self.paddle_x,
                    self.paddle_w,
                    paddle_top,
                    grid_left,
                    &mut self.bricks,
                )
            };
            if step.lost {
                self.balls[i] = None;
            }
            if let Some(idx) = step.brick {
                self.bricks_left -= 1;
                self.score += 10;
                if self.rand() % 100 < DROP_CHANCE {
                    let col = (idx % COLS) as f32;
                    let row = (idx / COLS) as f32;
                    self.spawn_drop(
                        grid_left + col * (BRICK_W + BRICK_GAP) + BRICK_W / 2.0,
                        BRICK_TOP + row * (BRICK_H + BRICK_GAP) + BRICK_H / 2.0,
                    );
                }
            }
        }

        // --- Falling collectibles ---
        self.update_drops();

        if self.bricks_left == 0 {
            self.drops = [None; MAX_DROPS];
            self.phase = Phase::LevelClear;
            return;
        }

        // --- Ball lost ---
        // A life is only lost when the *last* ball falls off the screen.
        if self.balls.iter().all(|b| b.is_none()) {
            self.lives -= 1;
            self.drops = [None; MAX_DROPS];
            self.paddle_w = self.difficulty.paddle_w(); // collectible effects do not survive a lost ball
            if self.lives == 0 {
                self.phase = Phase::GameOver;
            } else {
                self.phase = Phase::Ready;
                self.reset_balls();
            }
        }
    }

    /// Move the drops down, catch them with the paddle, discard missed ones.
    fn update_drops(&mut self) {
        let paddle_top = self.paddle_y();
        let half = DROP_SIZE / 2.0;
        for i in 0..MAX_DROPS {
            // Drop is Copy, so taking it out by value keeps `self` borrow-free
            // while we apply the effect below.
            let Some(mut d) = self.drops[i] else { continue };
            d.y += DROP_SPEED;
            let caught = d.y + half >= paddle_top
                && d.y - half <= paddle_top + PADDLE_H
                && d.x + half >= self.paddle_x - self.paddle_w / 2.0
                && d.x - half <= self.paddle_x + self.paddle_w / 2.0;
            if caught {
                self.drops[i] = None;
                self.apply_drop(d.kind);
            } else if d.y - half > self.height {
                self.drops[i] = None; // missed
            } else {
                self.drops[i] = Some(d);
            }
        }
    }

    fn apply_drop(&mut self, kind: DropKind) {
        match kind {
            DropKind::Grow => self.paddle_w = (self.paddle_w + PADDLE_RESIZE).min(PADDLE_MAX),
            DropKind::Shrink => self.paddle_w = (self.paddle_w - PADDLE_RESIZE).max(PADDLE_MIN),
            DropKind::MultiBall => self.spawn_extra_balls(),
        }
    }

    /// Queue a random collectible at a destroyed brick's center (the caller
    /// rolls the spawn chance).
    fn spawn_drop(&mut self, x: f32, y: f32) {
        let kind = match self.rand() % 100 {
            0..=34 => DropKind::Grow,
            35..=69 => DropKind::MultiBall,
            _ => DropKind::Shrink,
        };
        for slot in self.drops.iter_mut() {
            if slot.is_none() {
                *slot = Some(Drop { x, y, kind });
                return;
            }
        }
    }

    /// Split the first live ball into two extra copies rotated left/right
    /// (rotation keeps the speed constant). Capped at MAX_BALLS.
    fn spawn_extra_balls(&mut self) {
        let Some(src) = self.balls.iter().find_map(|b| *b) else {
            return;
        };
        for deg in [-25.0_f32, 25.0_f32] {
            if self.balls.iter().all(|b| b.is_some()) {
                return; // no free slot left
            }
            let a = deg.to_radians();
            let ball = Ball {
                x: src.x,
                y: src.y,
                dx: src.dx * math::cos(a) - src.dy * math::sin(a),
                dy: src.dx * math::sin(a) + src.dy * math::cos(a),
            };
            for slot in self.balls.iter_mut() {
                if slot.is_none() {
                    *slot = Some(ball);
                    break;
                }
            }
        }
    }

    /// Xorshift32.
    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    pub fn render(&self, screen: &mut Screen) {
        if self.phase == Phase::Menu {
            self.render_menu(screen);
            return;
        }
        screen.fill(COL_BG);

        // Playfield walls (top, left, right).
        screen.rect(0, 0, self.width as u32, 4, COL_WALL);
        screen.rect(0, 0, 4, self.height as u32, COL_WALL);
        screen.rect(self.width as u32 - 4, 0, 4, self.height as u32, COL_WALL);

        // Bricks.
        let grid_left = self.grid_left();
        for row in 0..ROWS {
            for col in 0..COLS {
                if !self.bricks[row * COLS + col] {
                    continue;
                }
                let x = grid_left + col as f32 * (BRICK_W + BRICK_GAP);
                let y = BRICK_TOP + row as f32 * (BRICK_H + BRICK_GAP);
                screen.rect_f(x, y, BRICK_W, BRICK_H, ROW_COLORS[row]);
            }
        }

        // Falling collectibles: a colored square with a letter.
        for d in self.drops.iter().flatten() {
            let (color, label) = match d.kind {
                DropKind::Grow => (COL_DROP_GROW, "G"),
                DropKind::Shrink => (COL_DROP_SHRINK, "S"),
                DropKind::MultiBall => (COL_DROP_MULTI, "M"),
            };
            let half = DROP_SIZE / 2.0;
            screen.rect_f(d.x - half, d.y - half, DROP_SIZE, DROP_SIZE, color);
            screen.text((d.x - 4.0) as u32, (d.y - 4.0) as u32, label, COL_BALL);
        }

        // Paddle.
        screen.rect_f(
            self.paddle_x - self.paddle_w / 2.0,
            self.paddle_y(),
            self.paddle_w,
            PADDLE_H,
            COL_PADDLE,
        );

        // Balls.
        for ball in self.balls.iter().flatten() {
            screen.circle(ball.x, ball.y, BALL_R, COL_BALL);
        }

        // HUD.
        screen.text_num(12, 12, "LIVES ", self.lives, COL_TEXT);
        let score_w = draw::num_width("SCORE ", self.score);
        screen.text_num(self.width as u32 - 12 - score_w, 12, "SCORE ", self.score, COL_TEXT);

        match self.phase {
            Phase::Menu => {}
            Phase::Ready => {
                screen.text_centered(self.height as u32 / 2 - 8, "CLICK OR PRESS SPACE TO LAUNCH", COL_TEXT);
            }
            Phase::LevelClear => {
                screen.text_centered(self.height as u32 / 2 - 16, "LEVEL CLEAR!", COL_TEXT);
                screen.text_centered(self.height as u32 / 2 + 4, "CLICK TO PLAY AGAIN", COL_TEXT);
            }
            Phase::GameOver => {
                screen.text_centered(self.height as u32 / 2 - 16, "GAME OVER", COL_TEXT);
                screen.text_centered(self.height as u32 / 2 + 4, "CLICK TO RETURN TO MENU", COL_TEXT);
            }
            Phase::Playing => {}
        }
    }

    /// Y coordinate of the top of menu entry `i`.
    fn menu_item_y(&self, i: usize) -> f32 {
        self.height / 2.0 - 60.0 + i as f32 * (MENU_ITEM_H + MENU_GAP)
    }

    /// Which menu entry (if any) contains the point (x, y)?
    fn menu_hit(&self, x: f32, y: f32) -> Option<usize> {
        for i in 0..MENU_ITEMS.len() {
            let top = self.menu_item_y(i);
            if x >= self.width / 2.0 - MENU_W / 2.0
                && x <= self.width / 2.0 + MENU_W / 2.0
                && y >= top
                && y <= top + MENU_ITEM_H
            {
                return Some(i);
            }
        }
        None
    }

    fn render_menu(&self, screen: &mut Screen) {
        screen.fill(COL_BG);

        screen.text_centered(self.height as u32 / 2 - 130, "BRICKSHOT", COL_TEXT);
        screen.text_centered(self.height as u32 / 2 - 110, "SELECT LEVEL", COL_DIM);

        for (i, (label, _)) in MENU_ITEMS.iter().enumerate() {
            let top = self.menu_item_y(i);
            let selected = i == self.menu_sel;
            let bg = if selected { COL_MENU_SEL } else { COL_MENU_ITEM };
            screen.rect_f(
                self.width / 2.0 - MENU_W / 2.0,
                top,
                MENU_W,
                MENU_ITEM_H,
                bg,
            );
            let label_color = if selected { COL_BALL } else { COL_TEXT };
            let x = (self.width / 2.0 - (label.len() as f32) * 4.0) as u32;
            screen.text(x, (top + 9.0) as u32, label, label_color);
        }

        screen.text_centered(
            self.height as u32 / 2 + 150,
            "CLICK OR PRESS SPACE TO SELECT - ARROWS TO MOVE - ESC TO QUIT",
            COL_DIM,
        );

        // Virtual cursor dot: with a relative (PS/2) mouse the position is
        // otherwise invisible, and hovering is guesswork.
        screen.rect_f(self.cursor_x - 2.0, self.cursor_y - 2.0, 4.0, 4.0, COL_BALL);
    }
}

/// Advance one ball by one frame and report what happened.
///
/// A free function (not a method) so the caller can hand it disjoint borrows
/// (`&mut Ball` out of the ball array plus `&mut bricks`) and keep using the
/// rest of `self` afterwards. At most one brick is consumed per ball per
/// frame, which keeps the bounce direction predictable.
fn step_ball(
    ball: &mut Ball,
    width: f32,
    height: f32,
    paddle_x: f32,
    paddle_w: f32,
    paddle_top: f32,
    grid_left: f32,
    bricks: &mut [bool; COLS * ROWS],
) -> BallStep {
    ball.x += ball.dx;
    ball.y += ball.dy;

    // Side walls and ceiling.
    if ball.x < BALL_R {
        ball.x = BALL_R;
        ball.dx = -ball.dx;
    }
    if ball.x > width - BALL_R {
        ball.x = width - BALL_R;
        ball.dx = -ball.dx;
    }
    if ball.y < BALL_R {
        ball.y = BALL_R;
        ball.dy = -ball.dy;
    }

    // Paddle collision. A ball whose bottom edge crossed the paddle's top
    // during this frame's move bounces off the top surface. A ball that
    // slid in from the side (already below the top edge when it came into
    // range) bounces off the side instead — snapping it up onto the top
    // would make it visibly "climb" the paddle.
    let half_w = paddle_w / 2.0;
    if ball.dy > 0.0
        && ball.x >= paddle_x - half_w - BALL_R
        && ball.x <= paddle_x + half_w + BALL_R
    {
        let bottom = ball.y + BALL_R;
        if bottom >= paddle_top && bottom - ball.dy <= paddle_top {
            // Top hit: the bounce angle depends on where the ball lands.
            ball.y = paddle_top - BALL_R;
            let hit = ((ball.x - paddle_x) / half_w).clamp(-0.95, 0.95);
            let angle = hit * 60.0_f32.to_radians();
            ball.dx = BALL_SPEED * math::sin(angle);
            ball.dy = -BALL_SPEED * math::cos(angle);
        } else if bottom > paddle_top && bottom <= paddle_top + PADDLE_H {
            // Side hit: reflect away from the paddle's center. A ball with
            // no horizontal speed gets a small nudge so it cannot fall
            // straight through the paddle's side.
            let min_dx = BALL_SPEED * 0.2;
            ball.dx = if ball.x < paddle_x {
                -ball.dx.abs().max(min_dx)
            } else {
                ball.dx.abs().max(min_dx)
            };
        }
    }

    // Brick collision: first hit wins.
    let mut brick = None;
    for (idx, alive) in bricks.iter_mut().enumerate() {
        if !*alive {
            continue;
        }
        let bx = grid_left + (idx % COLS) as f32 * (BRICK_W + BRICK_GAP);
        let by = BRICK_TOP + (idx / COLS) as f32 * (BRICK_H + BRICK_GAP);
        if ball.x + BALL_R >= bx
            && ball.x - BALL_R <= bx + BRICK_W
            && ball.y + BALL_R >= by
            && ball.y - BALL_R <= by + BRICK_H
        {
            *alive = false;
            // Reflect on the axis with the smaller penetration.
            let overlap_x = (ball.x - (bx + BRICK_W / 2.0)).abs() - BRICK_W / 2.0;
            let overlap_y = (ball.y - (by + BRICK_H / 2.0)).abs() - BRICK_H / 2.0;
            if overlap_x > overlap_y {
                ball.dx = -ball.dx;
            } else {
                ball.dy = -ball.dy;
            }
            brick = Some(idx);
            break;
        }
    }

    BallStep {
        lost: ball.y - BALL_R > height,
        brick,
    }
}
