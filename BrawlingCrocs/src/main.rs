use std::char::MAX;

use assets_manager::{asset::Png, AssetCache};
use frenderer::{
    input::{Input, Key},
    sprites::{Camera2D, SheetRegion, Transform},
    wgpu, Renderer,
};
mod geom;
mod grid;
use geom::*;
mod level;
use level::{EntityType, Level};
use rand::Rng;

const GRAV_ACC: f32 = 300.0;
const WALK_ACC: f32 = 180.0;
const MAX_SPEED: f32 = 90.0;
const BRAKE_DAMP: f32 = 0.9;
const JUMP_VEL: f32 = 140.0;
const JUMP_TIME_MAX: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    E,
    W,
}

impl Dir {
    fn to_vec2(self) -> Vec2 {
        Vec2 {
            x: match self {
                Dir::E => 1.0,
                Dir::W => -1.0,
            },
            y: 0.0,
        }
    }
}
struct Game {
    current_level: usize,
    levels: Vec<Level>,
    player1: Player,
    player2: Player,
    doors: Vec<(String, (u16, u16), Vec2)>,
    camera: Camera2D,
    animations: Vec<Animation>,
}

struct Contact {
    a_index: usize,
    a_rect: Rect,
    b_index: usize,
    b_rect: Rect,
    displacement: Vec2,
}

struct Player {
    pos: Vec2,
    vel: Vec2,
    dir: Dir,
    touching_door: bool,
    anim: AnimationState,
    jumping: bool,
    jump_timer: f32,
    grounded: bool,
    controls: Vec<Key>,
}
impl Player {
    fn rect(&self) -> Rect {
        Rect {
            x: self.pos.x
                + match self.dir {
                    Dir::E => 8.0,
                    Dir::W => 12.0,
                },
            y: self.pos.y - 12.0,
            w: 16,
            h: 24,
        }
    }
    fn trf(&self) -> Transform {
        Transform {
            w: 36,
            h: 36,
            x: self.pos.x + (self.dir.to_vec2().x * 8.0) + 18.0,
            y: self.pos.y - 12.0 + 18.0,
            rot: 0.0,
        }
    }
}

mod animation;
use animation::Animation;

#[allow(unused)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum AnimationKey {
    Blank = 0,
    PlayerRightIdle,
    PlayerRightWalk,
    PlayerRightJumpRise,
    PlayerRightJumpFall,
    PlayerRightAttack,
    PlayerLeftIdle,
    PlayerLeftWalk,
    PlayerLeftJumpRise,
    PlayerLeftJumpFall,
    PlayerLeftAttack,
}
struct AnimationState {
    animation: AnimationKey,
    t: f32,
}
#[allow(unused)]
impl AnimationState {
    fn finished(&self, anims: &[Animation]) -> bool {
        anims[self.animation as usize].sample(self.t).is_none()
    }
    fn sample(&self, anims: &[Animation]) -> SheetRegion {
        anims[self.animation as usize]
            .sample(self.t)
            .unwrap_or_else(|| anims[self.animation as usize].sample(0.0).unwrap())
    }
    fn tick(&mut self, dt: f32) {
        self.t += dt;
    }
    fn play(&mut self, anim: AnimationKey, retrigger: bool) {
        if anim == self.animation && !retrigger {
            return;
        }
        self.animation = anim;
        self.t = 0.0;
    }
}
// Feel free to change this if you use a different tilesheet
const TILE_SZ: usize = 16;
const W: usize = 16 * TILE_SZ;
const H: usize = 12 * TILE_SZ;
const SCREEN_FAST_MARGIN: f32 = 64.0;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    let source =
        assets_manager::source::FileSystem::new("content").expect("Couldn't load resources");
    #[cfg(target_arch = "wasm32")]
    let source = assets_manager::source::Embedded::from(assets_manager::source::embed!("content"));
    let cache = assets_manager::AssetCache::with_source(source);

    let drv = frenderer::Driver::new(
        winit::window::WindowBuilder::new()
            .with_title("test")
            .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0)),
        Some((W as u32, H as u32)),
    );

    const DT: f32 = 1.0 / 60.0;
    let mut input = Input::default();

    let mut now = frenderer::clock::Instant::now();
    let mut acc = 0.0;
    drv.run_event_loop::<(), _>(
        move |window, mut frend| {
            let game = Game::new(&mut frend, &cache);
            (window, game, frend)
        },
        move |event, target, (window, ref mut game, ref mut frend)| {
            use winit::event::{Event, WindowEvent};
            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    target.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(size),
                    ..
                } => {
                    if !frend.gpu.is_web() {
                        frend.resize_surface(size.width, size.height);
                    }
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {
                    let elapsed = now.elapsed().as_secs_f32();
                    // You can add the time snapping/death spiral prevention stuff here if you want.
                    // I'm not using it here to keep the starter code small.
                    acc += elapsed;
                    now = std::time::Instant::now();
                    // While we have time to spend
                    while acc >= DT {
                        // simulate a frame
                        acc -= DT;
                        game.simulate(&input, DT);
                        input.next_frame();
                    }
                    game.render(frend);
                    frend.render();
                    window.request_redraw();
                }
                event => {
                    input.process_input_event(&event);
                }
            }
        },
    )
    .expect("event loop error");
}

impl Game {
    fn new(renderer: &mut Renderer, cache: &AssetCache) -> Self {
        let tile_handle = cache
            .load::<Png>("tileset")
            .expect("Couldn't load tilesheet img");
        let tile_img = tile_handle.read().0.to_rgba8();
        let tile_tex = renderer.create_array_texture(
            &[&tile_img],
            wgpu::TextureFormat::Rgba8UnormSrgb,
            tile_img.dimensions(),
            Some("tiles-sprites"),
        );
        let levels = vec![
            Level::from_str(
                &cache
                    .load::<String>("level1")
                    .expect("Couldn't access level1.txt")
                    .read(),
            ),
            Level::from_str(
                &cache
                    .load::<String>("house")
                    .expect("Couldn't access house.txt")
                    .read(),
            ),
            Level::from_str(
                &cache
                    .load::<String>("shop")
                    .expect("Couldn't access shop.txt")
                    .read(),
            ),
        ];
        let current_level = 0;
        let camera = Camera2D {
            screen_pos: [0.0, 0.0],
            screen_size: [W as f32, H as f32],
        };
        let sprite_estimate =
            levels[current_level].sprite_count() + levels[current_level].starts().len();
        renderer.sprite_group_add(
            &tile_tex,
            vec![Transform::ZERO; sprite_estimate],
            vec![SheetRegion::ZERO; sprite_estimate],
            camera,
        );
        let player1_start = levels[current_level]
            .starts()
            .iter()
            .find(|(t, _)| *t == EntityType::Player)
            .map(|(_, ploc)| *ploc + Vec2 { x: 0.0, y: 200.0 })
            .expect("Start level doesn't put the player1 anywhere");
        let player2_start = levels[current_level]
            .starts()
            .iter()
            .find(|(t, _)| *t == EntityType::Player)
            .map(|(_, ploc)| *ploc + Vec2 { x: 100.0, y: 0.0 })
            .expect("Start level doesn't put the player2 anywhere");
        let mut game = Game {
            current_level,
            camera,
            levels,
            doors: vec![],
            player1: Player {
                vel: Vec2 { x: 0.0, y: 0.0 },
                pos: player1_start,
                dir: Dir::E,
                anim: AnimationState {
                    animation: AnimationKey::PlayerRightIdle,
                    t: 0.0,
                },
                touching_door: false,
                jumping: false,
                jump_timer: 0.0,
                grounded: true,
                controls: vec![Key::KeyW, Key::KeyA, Key::KeyS, Key::KeyD],
            },
            player2: Player {
                vel: Vec2 { x: 0.0, y: 0.0 },
                pos: player2_start,
                dir: Dir::E,
                anim: AnimationState {
                    animation: AnimationKey::PlayerRightIdle,
                    t: 0.0,
                },
                touching_door: false,
                jumping: false,
                jump_timer: 0.0,
                grounded: true,
                controls: vec![Key::KeyI, Key::KeyJ, Key::KeyK, Key::KeyL],
            },
            animations: vec![
                Animation::with_frame(SheetRegion::ZERO),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(0, 300, 36, 36),
                        SheetRegion::rect(60, 300, 36, 36),
                        SheetRegion::rect(120, 300, 36, 36),
                        SheetRegion::rect(180, 300, 36, 36),
                        SheetRegion::rect(300, 300, 36, 36),
                    ],
                    0.15,
                )
                .looped(),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(300, 300, 36, 36),
                        SheetRegion::rect(360, 300, 36, 36),
                        SheetRegion::rect(420, 300, 36, 36),
                    ],
                    0.15,
                )
                .looped(),
                Animation::with_frame(SheetRegion::rect(480, 300, 36, 36)),
                Animation::with_frame(SheetRegion::rect(539, 300, 36, 36)),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(779, 300, 36, 36),
                        SheetRegion::rect(839, 300, 36, 36),
                        SheetRegion::rect(899, 300, 36, 36),
                        SheetRegion::rect(959, 300, 36, 36),
                        SheetRegion::rect(1019, 300, 36, 36),
                        SheetRegion::rect(1079, 300, 36, 36),
                        SheetRegion::rect(1139, 300, 36, 36),
                    ],
                    0.07,
                ),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(0, 300, 36, 36),
                        SheetRegion::rect(60, 300, 36, 36),
                        SheetRegion::rect(120, 300, 36, 36),
                        SheetRegion::rect(180, 300, 36, 36),
                        SheetRegion::rect(300, 300, 36, 36),
                    ],
                    0.15,
                )
                .looped()
                .flip_horizontal(),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(300, 300, 36, 36),
                        SheetRegion::rect(360, 300, 36, 36),
                        SheetRegion::rect(420, 300, 36, 36),
                    ],
                    0.15,
                )
                .looped()
                .flip_horizontal(),
                Animation::with_frame(SheetRegion::rect(480, 300, 36, 36)).flip_horizontal(),
                Animation::with_frame(SheetRegion::rect(539, 300, 36, 36)).flip_horizontal(),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(779, 300, 36, 36),
                        SheetRegion::rect(839, 300, 36, 36),
                        SheetRegion::rect(899, 300, 36, 36),
                        SheetRegion::rect(959, 300, 36, 36),
                        SheetRegion::rect(1019, 300, 36, 36),
                        SheetRegion::rect(1079, 300, 36, 36),
                        SheetRegion::rect(1139, 300, 36, 36),
                    ],
                    0.07,
                )
                .flip_horizontal(),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(0, 272, 36, 16),
                        SheetRegion::rect(36, 272, 36, 16),
                        SheetRegion::rect(36 * 2, 272, 36, 16),
                        SheetRegion::rect(36 * 3, 272, 36, 16),
                        SheetRegion::rect(36 * 4, 272, 36, 16),
                        SheetRegion::rect(36 * 5, 272, 36, 16),
                        SheetRegion::rect(36 * 6, 272, 36, 16),
                        SheetRegion::rect(36 * 7, 272, 36, 16),
                        //SheetRegion::rect(36 * 8, 272, 36, 16),
                    ],
                    0.1,
                )
                .looped(),
                Animation::with_frames(
                    &[
                        SheetRegion::rect(0, 272, 36, 16),
                        SheetRegion::rect(36, 272, 36, 16),
                        SheetRegion::rect(36 * 2, 272, 36, 16),
                        SheetRegion::rect(36 * 3, 272, 36, 16),
                        SheetRegion::rect(36 * 4, 272, 36, 16),
                        SheetRegion::rect(36 * 5, 272, 36, 16),
                        SheetRegion::rect(36 * 6, 272, 36, 16),
                        SheetRegion::rect(36 * 7, 272, 36, 16),
                        //SheetRegion::rect(36 * 8, 272, 36, 16),
                    ],
                    0.1,
                )
                .looped()
                .flip_horizontal(),
            ],
        };
        game.enter_level(player1_start, player2_start);
        game
    }
    fn level(&self) -> &Level {
        &self.levels[self.current_level]
    }
    fn enter_level(&mut self, player1_pos: Vec2, player2_pos: Vec2) {
        self.doors.clear();
        // we will probably enter at a door
        self.player1.touching_door = true;
        self.player1.pos = player1_pos;
        self.player2.touching_door = true;
        self.player2.pos = player2_pos;
        for (etype, pos) in self.levels[self.current_level].starts().iter() {
            match etype {
                EntityType::Player => {}
                EntityType::Door(rm, x, y) => self.doors.push((rm.clone(), (*x, *y), *pos)),
            }
        }
    }
    fn sprite_count(&self) -> usize {
        //todo!("count how many entities and other sprites we have");
        self.level().sprite_count() + /*self.enemies.len() +*/ 2
    }
    fn render(&mut self, frend: &mut Renderer) {
        // make this exactly as big as we need
        frend.sprite_group_resize(0, self.sprite_count());
        frend.sprite_group_set_camera(0, self.camera);

        let sprites_used = self.level().render_into(frend, 0);
        let (sprite_posns, sprite_gfx) = frend.sprites_mut(0, sprites_used..);

        sprite_posns[0] = self.player1.trf();
        sprite_gfx[0] = self.player1.anim.sample(&self.animations);
        sprite_posns[1] = self.player2.trf();
        sprite_gfx[1] = self.player2.anim.sample(&self.animations);
    }
    fn simulate(&mut self, input: &Input, dt: f32) {
        // Player 1 acceleration
        let acc1 = Vec2 {
            x: WALK_ACC * input.key_axis(self.player1.controls[1], self.player1.controls[3]),
            y: GRAV_ACC,
        };

        // Player 2 acceleration
        let acc2 = Vec2 {
            x: WALK_ACC * input.key_axis(self.player2.controls[1], self.player2.controls[3]),
            y: GRAV_ACC,
        };

        // Player 1 directions
        if input.is_key_down(self.player1.controls[3]) {
            self.player1.dir = Dir::E;
            self.player1.anim.play(AnimationKey::PlayerRightWalk, false);
        } else if input.is_key_down(self.player1.controls[1]) {
            self.player1.dir = Dir::W;
            self.player1.anim.play(AnimationKey::PlayerLeftWalk, false);
        }

        // Player 2 directions and animations
        if input.is_key_down(self.player2.controls[3]) {
            self.player2.dir = Dir::E;
            self.player2.anim.play(AnimationKey::PlayerRightWalk, false);
        } else if input.is_key_down(self.player2.controls[1]) {
            self.player2.dir = Dir::W;
            self.player2.anim.play(AnimationKey::PlayerLeftWalk, false);
        }

        // Player 1 jumping
        if input.is_key_down(self.player1.controls[0]) && self.player1.grounded {
            self.player1.jumping = true;
            self.player1.jump_timer = JUMP_TIME_MAX;
            self.player1.vel.y = JUMP_VEL;
        }

        // Player 2 jumping
        if input.is_key_down(self.player2.controls[0]) && self.player2.grounded {
            self.player2.jumping = true;
            self.player2.jump_timer = JUMP_TIME_MAX;
            self.player2.vel.y = JUMP_VEL;
        }

        // Player 1 jump timer
        if self.player1.jumping {
            self.player1.jump_timer -= dt;
            if self.player1.jump_timer <= 0.0 {
                self.player1.jumping = false;
            }
        }

        // Player 2 jump timer
        if self.player2.jumping {
            self.player2.jump_timer -= dt;
            if self.player2.jump_timer <= 0.0 {
                self.player2.jumping = false;
            }
        }

        // Player 1 movement and physics
        self.player1.vel.y -= GRAV_ACC * dt;
        self.player1.vel.x += acc1.x * dt;
        if acc1.x.abs() < 0.1 {
            self.player1.vel.x *= BRAKE_DAMP;
        }
        self.player1.vel.x = self.player1.vel.x.clamp(-MAX_SPEED, MAX_SPEED);

        self.player1.pos += self.player1.vel * dt;

        let lw = self.level().width();
        let lh = self.level().height();
        self.player1.pos.x = self.player1.pos.x.clamp(
            0.0,
            lw as f32 * TILE_SZ as f32 - self.player1.rect().w as f32 / 2.0,
        );
        self.player1.pos.y = self
            .player1
            .pos
            .y
            .clamp(0.0, H as f32 - self.player1.rect().h as f32);

        self.player1.grounded = self.player1.pos.y <= 0.0;
        if self.player1.grounded {
            self.player1.vel.y = 0.0;
        }

        // Player 2 movement and physics
        self.player2.vel.y -= GRAV_ACC * dt;
        self.player2.vel.x += acc2.x * dt;
        if acc2.x.abs() < 0.1 {
            self.player2.vel.x *= BRAKE_DAMP;
        }
        self.player2.vel.x = self.player2.vel.x.clamp(-MAX_SPEED, MAX_SPEED);

        self.player2.pos += self.player2.vel * dt;

        self.player2.pos.x = self.player2.pos.x.clamp(
            0.0,
            lw as f32 * TILE_SZ as f32 - self.player2.rect().w as f32 / 2.0,
        );
        self.player2.pos.y = self
            .player2
            .pos
            .y
            .clamp(0.0, H as f32 - self.player2.rect().h as f32);

        self.player2.grounded = self.player2.pos.y <= 0.0;
        if self.player2.grounded {
            self.player2.vel.y = 0.0;
        }

        // Player 1 jump animation
        if self.player1.jumping {
            match self.player1.dir {
                Dir::E => self
                    .player1
                    .anim
                    .play(AnimationKey::PlayerRightJumpRise, false),
                Dir::W => self
                    .player1
                    .anim
                    .play(AnimationKey::PlayerLeftJumpRise, false),
            }
        } else {
            match self.player1.dir {
                Dir::E => self.player1.anim.play(AnimationKey::PlayerRightIdle, false),
                Dir::W => self.player1.anim.play(AnimationKey::PlayerLeftIdle, false),
            }
        }

        self.player1.anim.tick(dt);

        // Player 2 jump animation
        if self.player2.jumping {
            match self.player2.dir {
                Dir::E => self
                    .player2
                    .anim
                    .play(AnimationKey::PlayerRightJumpRise, false),
                Dir::W => self
                    .player2
                    .anim
                    .play(AnimationKey::PlayerLeftJumpRise, false),
            }
        } else {
            match self.player2.dir {
                Dir::E => self.player2.anim.play(AnimationKey::PlayerRightIdle, false),
                Dir::W => self.player2.anim.play(AnimationKey::PlayerLeftIdle, false),
            }
        }

        self.player2.anim.tick(dt);

        // Door collision and response
        let mut triggers = vec![];
        let p1rect = self.player1.rect();
        let p2rect = self.player2.rect();

        let mut player1_tile_contacts = vec![];
        let mut player2_tile_contacts = vec![];
        self::Game::gather_contacts_tiles(&[p1rect], self.level(), &mut player1_tile_contacts);
        self::Game::gather_contacts_tiles(&[p2rect], self.level(), &mut player2_tile_contacts);

        self.player1.grounded = false;
        for contact in player1_tile_contacts {
            let disp = Self::compute_disp(self.player1.rect(), contact.b_rect);
            self.player1.pos += disp;
            if disp.y > 0.0 {
                self.player1.grounded = true;
                self.player1.vel.y = 0.0;
            }
        }

        self.player2.grounded = false;
        for contact in player2_tile_contacts {
            let disp = Self::compute_disp(self.player2.rect(), contact.b_rect);
            self.player2.pos += disp;
            if disp.y > 0.0 {
                self.player2.grounded = true;
                self.player2.vel.y = 0.0;
            }
        }

        let door_rects = self
            .doors
            .iter()
            .map(|(_, _, pos)| Rect {
                x: pos.x - 8.0,
                y: pos.y - 8.0,
                w: 16,
                h: 16,
            })
            .collect::<Vec<_>>();
        self::Game::gather_contacts(&[p1rect], &door_rects, &mut triggers);
        if triggers.is_empty() {
            self.player1.touching_door = false;
        }
        for Contact { b_index: door, .. } in triggers.drain(..) {
            // enter door if player1 has moved, wasn't previously touching door
            if !self.player1.touching_door {
                self.player1.touching_door = true;
                let (door_to, door_to_pos, _door_pos) = &self.doors[door];
                let dest = self
                    .levels
                    .iter()
                    .position(|l| l.name() == door_to)
                    .expect("door to invalid room {door_to}!");
                if dest == self.current_level {
                    self.player1.pos = self
                        .level()
                        .grid_to_world((door_to_pos.0 as usize, door_to_pos.1 as usize))
                        + Vec2 {
                            x: TILE_SZ as f32 / 2.0,
                            y: -12.0 + TILE_SZ as f32 / 2.0,
                        };
                } else {
                    self.current_level = dest;
                    self.enter_level(
                        self.level()
                            .grid_to_world((door_to_pos.0 as usize, door_to_pos.1 as usize))
                            + Vec2 {
                                x: 0.0 + TILE_SZ as f32 / 2.0,
                                y: -12.0 + TILE_SZ as f32 / 2.0,
                            },
                        self.level()
                            .grid_to_world((door_to_pos.0 as usize, door_to_pos.1 as usize))
                            + Vec2 {
                                x: 0.0 + TILE_SZ as f32 / 2.0,
                                y: -12.0 + TILE_SZ as f32 / 2.0,
                            },
                    );
                }
                break;
            }
        }
        while self.player1.pos.x
            > self.camera.screen_pos[0] + self.camera.screen_size[0] - SCREEN_FAST_MARGIN
        {
            self.camera.screen_pos[0] += 1.0;
        }
        while self.player1.pos.x < self.camera.screen_pos[0] + SCREEN_FAST_MARGIN {
            self.camera.screen_pos[0] -= 1.0;
        }
        while self.player1.pos.y
            > self.camera.screen_pos[1] + self.camera.screen_size[1] - SCREEN_FAST_MARGIN
        {
            self.camera.screen_pos[1] += 1.0;
        }
        while self.player1.pos.y < self.camera.screen_pos[1] + SCREEN_FAST_MARGIN {
            self.camera.screen_pos[1] -= 1.0;
        }
        self.camera.screen_pos[0] =
            self.camera.screen_pos[0].clamp(0.0, (lw * TILE_SZ).max(W) as f32 - W as f32);
        self.camera.screen_pos[1] =
            self.camera.screen_pos[1].clamp(0.0, (lh * TILE_SZ).max(H) as f32 - H as f32);
    }

    fn gather_contacts_tiles(rects: &[Rect], level: &Level, contacts: &mut Vec<Contact>) {
        for (rect_i, rect) in rects.iter().enumerate() {
            for (tr, _td) in level.tiles_within(*rect).filter(|(_, td)| td.solid) {
                if let Some(displacement) = rect.overlap(tr) {
                    contacts.push(Contact {
                        a_index: rect_i,
                        a_rect: *rect,
                        b_index: 0,
                        b_rect: tr,
                        displacement,
                    });
                }
            }
        }
    }

    fn gather_contacts(rects_a: &[Rect], rects_b: &[Rect], contacts: &mut Vec<Contact>) {
        for (a_i, a) in rects_a.iter().enumerate() {
            for (b_i, b) in rects_b.iter().enumerate() {
                if let Some(displacement) = a.overlap(*b) {
                    contacts.push(Contact {
                        a_index: a_i,
                        a_rect: *a,
                        b_index: b_i,
                        b_rect: *b,
                        displacement,
                    });
                }
            }
        }
    }

    fn compute_disp(a: Rect, b: Rect) -> Vec2 {
        let mut displacement = a.overlap(b).unwrap_or(Vec2 { x: 0.0, y: 0.0 });
        if displacement.x < displacement.y {
            displacement.y = 0.0;
        } else {
            displacement.x = 0.0;
        }
        if a.x < b.x {
            displacement.x = -displacement.x;
        }
        if a.y < b.y {
            displacement.y = -displacement.y;
        }
        displacement
    }
}
