use assets_manager::{asset::Png, AssetCache};
use frenderer::bitfont::BitFont;
use frenderer::{
    input::{Input, Key},
    sprites::{Camera2D, SheetRegion, Transform},
    wgpu, Renderer,
};
use std::cmp::max;
use std::io::{stdout, Write};
mod geom;
mod grid;
use geom::*;
mod level;
use level::{EntityType, Level};
use rand::Rng;

use rodio::{source::Source, Decoder, OutputStream};
use std::fs::File;
use std::io::BufReader;

const GRAV_ACC: f32 = 400.0;
const WALK_ACC: f32 = 180.0;
const WALK_VEL: f32 = 90.0;
const MUD_COEFF: f32 = 0.5;
const MAX_SPEED: f32 = 90.0;
const BRAKE_DAMP: f32 = 0.9;
const JUMP_VEL: f32 = 120.0;
const JUMP_TIME_MAX: f32 = 0.15;
const ATTACK_MAX_TIME: f32 = 0.6;
const ATTACK_COOLDOWN_TIME: f32 = 0.1;

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
    enemies: Vec<Enemy>,
    mud_puddles: Vec<Mud>,
    camera: Camera2D,
    animations: Vec<Animation>,
}

struct Mud {
    pos: Vec2,
}

struct Enemy {
    pos: Vec2,
    dir: Dir,
    vel: Vec2,
    dead: bool,
    change_dir_timer: f32,
    anim: AnimationState,
}
impl Enemy {
    fn die(&mut self) {
        self.dead = true;
    }
    fn rect(&self) -> Rect {
        if self.dead {
            return Rect::ZERO;
        }
        Rect {
            x: self.pos.x - 18.0 + 6.0,
            y: self.pos.y - 8.0,
            w: 24,
            h: 8,
        }
    }
    fn trf(&self) -> Transform {
        if self.dead {
            return Transform::ZERO;
        }
        Transform {
            w: 36,
            h: 16,
            x: self.pos.x,
            y: self.pos.y,
            rot: 0.0,
        }
    }
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
    touching_mud: bool,
    anim: AnimationState,
    jumping: bool,
    jump_timer: f32,
    grounded: bool,
    attack_timer: f32,
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
    fn attack_rect(&self) -> Rect {
        Rect {
            x: self.pos.x
                + match self.dir {
                    Dir::E => 16.0,
                    Dir::W => 0.0,
                },
            y: self.pos.y - 12.0,
            w: if self.attack_timer > ATTACK_COOLDOWN_TIME {
                18
            } else {
                0
            },
            h: if self.attack_timer > ATTACK_COOLDOWN_TIME {
                24
            } else {
                0
            },
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
    EnemyRightWalk,
    EnemyLeftWalk,
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
    // Get a output stream handle to the default physical sound device
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    // Load a sound from a file, using a path relative to Cargo.toml
    let file = BufReader::new(File::open("content/moosic.ogg").unwrap());
    // Decode that sound file into a source
    let source = Decoder::new(file).unwrap();
    // Play the sound directly on the device
    stream_handle.play_raw(source.convert_samples());

    // The sound plays in a separate audio thread,
    // so we need to keep the main thread alive while it's playing.
    std::thread::sleep(std::time::Duration::from_secs(5));

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
            // Level::from_str(
            //     &cache
            //         .load::<String>("house")
            //         .expect("Couldn't access house.txt")
            //         .read(),
            // ),
            // Level::from_str(
            //     &cache
            //         .load::<String>("shop")
            //         .expect("Couldn't access shop.txt")
            //         .read(),
            // ),
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
            .map(|(_, ploc)| *ploc + Vec2 { x: 0.0, y: 0.0 })
            .expect("Start level doesn't put the player2 anywhere");
        let mut game = Game {
            current_level,
            camera,
            levels,
            enemies: vec![],
            mud_puddles: vec![],
            player1: Player {
                vel: Vec2 {
                    x: WALK_VEL,
                    y: 0.0,
                },
                pos: player1_start,
                dir: Dir::E,
                anim: AnimationState {
                    animation: AnimationKey::PlayerRightIdle,
                    t: 0.0,
                },
                touching_mud: false,
                jumping: false,
                jump_timer: 0.0,
                grounded: true,
                attack_timer: 0.2,
                controls: vec![Key::Space],
            },
            player2: Player {
                vel: Vec2 {
                    x: WALK_VEL,
                    y: 0.0,
                },
                pos: player2_start,
                dir: Dir::E,
                anim: AnimationState {
                    animation: AnimationKey::PlayerRightIdle,
                    t: 0.0,
                },
                touching_mud: false,
                jumping: false,
                jump_timer: 0.0,
                grounded: true,
                attack_timer: 0.2,
                controls: vec![Key::ArrowUp],
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
        self.mud_puddles.clear();
        self.enemies.clear();
        self.player1.touching_mud = false;
        self.player1.pos = player1_pos;
        self.player2.touching_mud = false;
        self.player2.pos = player2_pos;
        for (etype, pos) in self.levels[self.current_level].starts().iter() {
            match etype {
                EntityType::Player => {}
                EntityType::Enemy => self.enemies.push(Enemy {
                    dead: false,
                    pos: *pos,
                    vel: Vec2 { x: 0.0, y: 0.0 },
                    dir: if rand::thread_rng().gen_bool(0.5) {
                        Dir::E
                    } else {
                        Dir::W
                    },
                    anim: AnimationState {
                        animation: AnimationKey::EnemyLeftWalk,
                        t: 0.0,
                    },
                    change_dir_timer: rand::thread_rng().gen_range(3.0..5.0),
                }),
                EntityType::Mud => self.mud_puddles.push(Mud { pos: *pos }),
            }
        }
    }
    fn sprite_count(&self) -> usize {
        //todo!("count how many entities and other sprites we have");
        self.level().sprite_count() + self.enemies.len() + 2
    }
    fn render(&mut self, frend: &mut Renderer) {
        // print!("BOOOO!!!");
        // make this exactly as big as we need
        frend.sprite_group_resize(0, self.sprite_count());
        frend.sprite_group_set_camera(0, self.camera);

        let sprites_used = self.level().render_into(frend, 0);
        let (sprite_posns, sprite_gfx) = frend.sprites_mut(0, sprites_used..);

        for (enemy, (trf, uv)) in self
            .enemies
            .iter()
            .zip(sprite_posns.iter_mut().zip(sprite_gfx.iter_mut()))
        {
            *trf = enemy.trf();
            *uv = self.animations[AnimationKey::EnemyLeftWalk as usize]
                .sample(0.0)
                .unwrap();
        }
        let sprite_posns = &mut sprite_posns[self.enemies.len()..];
        let sprite_gfx = &mut sprite_gfx[self.enemies.len()..];
        sprite_posns[0] = self.player1.trf();
        sprite_gfx[0] = self.player1.anim.sample(&self.animations);
        sprite_posns[1] = self.player2.trf();
        sprite_gfx[1] = self.player2.anim.sample(&self.animations);

        frend.sprite_group_set_camera(
            0,
            Camera2D {
                screen_pos: [
                    (max(self.player1.pos.x as usize, self.player2.pos.x as usize) as f32
                        - (W / 4) as f32) as f32,
                    7_f32,
                ],
                screen_size: [W as f32, H as f32],
            },
        );
    }
    fn simulate(&mut self, input: &Input, dt: f32) {
        //Player 1 acceleration
        let acc1 = Vec2 {
            x: 0.0,
            y: GRAV_ACC,
        };

        //Player 2 acceleration
        let acc2 = Vec2 {
            x: 0.0,
            y: GRAV_ACC,
        };

        // Player 1 directions
        // if input.is_key_down(self.player1.controls[0]) {
        //     self.player1.dir = Dir::E;
        //     self.player1.anim.play(AnimationKey::PlayerRightWalk, false);
        // } else if input.is_key_down(self.player1.controls[1]) {
        //     self.player1.dir = Dir::W;
        //     self.player1.anim.play(AnimationKey::PlayerLeftWalk, false);
        // }

        // Player 2 directions and animations
        // if input.is_key_down(self.player2.controls[3]) {
        //     self.player2.dir = Dir::E;
        //     self.player2.anim.play(AnimationKey::PlayerRightWalk, false);
        // } else if input.is_key_down(self.player2.controls[1]) {
        //     self.player2.dir = Dir::W;
        //     self.player2.anim.play(AnimationKey::PlayerLeftWalk, false);
        // }

        // println!("simulate");
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
        // self.player1.vel.x += acc1.x * dt;
        // if acc1.x.abs() < 0.1 {
        //     self.player1.vel.x *= BRAKE_DAMP;
        // }
        // self.player1.vel.x = self.player1.vel.x.clamp(-MAX_SPEED, MAX_SPEED);

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

        self.player1.grounded = self.player1.pos.y <= 140.0;
        if self.player1.grounded {
            self.player1.vel.y = 0.0;
        }

        // println!(
        //     "Player 1: pos: {:?}, vel: {:?}, grounded: {:?}",
        //     self.player1.pos, self.player1.vel, self.player1.grounded
        // );

        // Player 2 movement and physics
        self.player2.vel.y -= GRAV_ACC * dt;
        // self.player2.vel.x += acc2.x * dt;
        // if acc2.x.abs() < 0.1 {
        //     self.player2.vel.x *= BRAKE_DAMP;
        // }
        // self.player2.vel.x = self.player2.vel.x.clamp(-MAX_SPEED, MAX_SPEED);

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

        self.player2.grounded = self.player2.pos.y <= 44.0;
        if self.player2.grounded {
            self.player2.vel.y = 0.0;
        }

        // println!(
        //     "Player 2: pos: {:?}, vel: {:?}, grounded: {:?}",
        //     self.player2.pos, self.player2.vel, self.player2.grounded
        // );

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

        // Enemy calculations
        for enemy in self.enemies.iter_mut() {
            if enemy.dead {
                continue;
            }
            enemy.change_dir_timer -= dt;
            if enemy.change_dir_timer <= 0.0 {
                enemy.dir = if enemy.dir == Dir::E { Dir::W } else { Dir::E };
                enemy.change_dir_timer = rand::thread_rng().gen_range(3.0..5.0);
            }

            enemy.vel.x = MAX_SPEED
                * match enemy.dir {
                    Dir::E => 1.0,
                    Dir::W => -1.0,
                };
            enemy.pos += enemy.vel * dt;
            enemy.pos.x = enemy.pos.x.clamp(
                0.0,
                lw as f32 * TILE_SZ as f32 - enemy.rect().w as f32 / 2.0,
            );
            enemy.pos.y = enemy.pos.y.clamp(
                0.0,
                lh as f32 * TILE_SZ as f32 * H as f32 - enemy.rect().h as f32 / 2.0,
            );
            match enemy.dir {
                Dir::E => {
                    enemy.anim.play(AnimationKey::EnemyRightWalk, false);
                }
                Dir::W => {
                    enemy.anim.play(AnimationKey::EnemyLeftWalk, false);
                }
            }
            enemy.anim.tick(dt);
        }

        // Mud collision and response
        let mut p1_triggers = vec![];
        let mut p2_triggers = vec![];
        let p1rect = self.player1.rect();
        let p2rect = self.player2.rect();

        let enemy_rects = self
            .enemies
            .iter()
            .map(|enemy| Rect {
                x: enemy.pos.x,
                y: enemy.pos.y,
                w: 16,
                h: 16,
            })
            .collect::<Vec<_>>();

        let mut player1_tile_contacts = vec![];
        let mut player2_tile_contacts = vec![];
        let mut enemy_tile_contacts = vec![];

        self::Game::gather_contacts_tiles(&enemy_rects, self.level(), &mut enemy_tile_contacts);
        self::Game::gather_contacts_tiles(&[p1rect], self.level(), &mut player1_tile_contacts);
        self::Game::gather_contacts_tiles(&[p2rect], self.level(), &mut player2_tile_contacts);

        // self::Game::gather_contacts(&[p1rect], &enemy_rects, &mut contacts);
        // self::Game::gather_contacts(&[p2rect], &enemy_rects, &mut contacts);

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

        // Attacks
        let mut attack_contacts = vec![];

        if input.is_key_down(Key::Space) && self.player1.attack_timer > ATTACK_COOLDOWN_TIME {
            self.player1.anim.play(
                match self.player1.dir {
                    Dir::E => AnimationKey::PlayerRightAttack,
                    Dir::W => AnimationKey::PlayerLeftAttack,
                },
                false,
            );
            self.player1.attack_timer = 0.0;
        }

        if input.is_key_down(Key::Enter) && self.player2.attack_timer > ATTACK_COOLDOWN_TIME {
            self.player2.anim.play(
                match self.player2.dir {
                    Dir::E => AnimationKey::PlayerRightAttack,
                    Dir::W => AnimationKey::PlayerLeftAttack,
                },
                false,
            );
            self.player2.attack_timer = 0.0;
        }

        // self::Game::gather_contacts(&[attack_rect], &enemy_rects, &mut attack_contacts);
        self.attack_enemy_collision_response(&mut attack_contacts);

        for contact in enemy_tile_contacts {
            let disp = Self::compute_disp(contact.a_rect, contact.b_rect);
            self.enemies[contact.a_index].pos += disp;
        }

        let mud_rects = self
            .mud_puddles
            .iter()
            .map(|m| Rect {
                x: m.pos.x - 8.0,
                y: m.pos.y - 8.0,
                w: 16,
                h: 16,
            })
            .collect::<Vec<_>>();

        // remember: this is gathering contacts between the player1 and the mud puddles
        self::Game::gather_contacts(&[p1rect], &mud_rects, &mut p1_triggers);
        if p1_triggers.is_empty() {
            self.player1.touching_mud = false;
            self.player1.vel.x = WALK_VEL;
        }
        for Contact { b_index: _mud, .. } in p1_triggers.drain(..) {
            // enter mud if player1 has moved, wasn't previously touching mud
            if !self.player1.touching_mud {
                self.player1.touching_mud = true;
                self.player1.vel.x = WALK_VEL * MUD_COEFF;
                // println!("Player 1 is touching mud");

                break;
            }
        }

        // remember: this is gathering contacts between the player2 and the mud puddles
        self::Game::gather_contacts(&[p2rect], &mud_rects, &mut p2_triggers);
        if p2_triggers.is_empty() {
            self.player2.touching_mud = false;
            self.player2.vel.x = WALK_VEL;
        }
        for Contact { b_index: _mud, .. } in p2_triggers.drain(..) {
            // enter mud if player2 has moved, wasn't previously touching mud
            if !self.player2.touching_mud {
                self.player2.touching_mud = true;
                self.player2.vel.x = WALK_VEL * MUD_COEFF;
                // println!("Player 2 is touching mud");
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

    // computes the displacement to resolve the contact
    fn compute_disp(a: Rect, b: Rect) -> Vec2 {
        let mut displacement = a.overlap(b).unwrap_or(Vec2 { x: 0.0, y: 0.0 });
        if displacement.x < displacement.y {
            displacement.y = 0.0;
        } else {
            displacement.x = 0.0;
        }
        // rectangle a is left of rectangle b, displacement becomes negative
        // rectabnel a is bellow rectangle b, displacement becomes negative
        if a.x < b.x {
            displacement.x = -displacement.x;
        }
        if a.y < b.y {
            displacement.y = -displacement.y;
        }
        displacement
    }

    // collision response for the player1 and enemies, give the player1 knockback and lose one heart
    // fn player_enemy_collision_response(&mut self, contacts: &mut [Contact]) {
    //     for contact in contacts {
    //         if self.knockback_timer > 0.0 {
    //             continue;
    //         }
    //         self.knockback_timer = KNOCKBACK_TIME;
    //         self.health -= 1;
    //     }
    // }

    // collision response for the player1's attack and enemies, remove the enemy
    fn attack_enemy_collision_response(&mut self, contacts: &mut [Contact]) {
        let mut to_remove = vec![];
        for contact in contacts {
            to_remove.push(contact.b_index);
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for i in to_remove.iter().rev() {
            self.enemies.remove(*i);
        }
    }
}
