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

use rodio::{source::Source, Decoder, OutputStream};
use std::fs::File;
use std::io::BufReader;

const GRAV_ACC: f32 = 300.0;
const WALK_ACC: f32 = 180.0;
const MAX_SPEED: f32 = 90.0;
const BRAKE_DAMP: f32 = 0.9;
const JUMP_VEL: f32 = 140.0;
const JUMP_TIME_MAX: f32 = 0.25;
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
    player: Player,
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
    // Get a output stream handle to the default physical sound device
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    // Load a sound from a file, using a path relative to Cargo.toml
    let file = BufReader::new(File::open("content/gamemusic.ogg").unwrap());
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
        let player_start = levels[current_level]
            .starts()
            .iter()
            .find(|(t, _)| *t == EntityType::Player)
            .map(|(_, ploc)| *ploc + Vec2 { x: 0.0, y: 200.0 })
            .expect("Start level doesn't put the player anywhere");
        let mut game = Game {
            current_level,
            camera,
            levels,
            enemies: vec![],
            doors: vec![],
            player: Player {
                vel: Vec2 { x: 0.0, y: 0.0 },
                pos: player_start,
                dir: Dir::E,
                anim: AnimationState {
                    animation: AnimationKey::PlayerRightIdle,
                    t: 0.0,
                },
                touching_door: false,
                jumping: false,
                jump_timer: 0.0,
                grounded: true,
            },
            animations: vec![
                Animation::with_frame(SheetRegion::ZERO),
                Animation::with_frames(
                    &[
                        // PlayerRightIdle
                        SheetRegion::rect(0, 364, 36, 36),
                    ],
                    0.15,
                )
                .looped(),
                Animation::with_frames(
                    &[
                        // PlayerRightWalk
                        SheetRegion::rect(0, 364, 36, 36),
                        SheetRegion::rect(60, 364, 36, 36),
                        SheetRegion::rect(120, 364, 36, 36),
                        SheetRegion::rect(180, 364, 36, 36),
                        SheetRegion::rect(240, 364, 36, 36),
                    ],
                    0.10,
                )
                .looped(),
                // PlayerRightJumpRise
                Animation::with_frame(SheetRegion::rect(60, 364, 36, 36)),
                // PlayerRightJumpFall
                Animation::with_frame(SheetRegion::rect(180, 364, 36, 36)),
                Animation::with_frames(
                    &[
                        // PlayerRightAttack
                        SheetRegion::rect(120, 364, 36, 36),
                    ],
                    0.07,
                ),
                Animation::with_frames(
                    &[
                        // PlayerLeftIdle
                        SheetRegion::rect(0, 364, 36, 36),
                    ],
                    0.15,
                )
                .looped()
                .flip_horizontal(),
                Animation::with_frames(
                    &[
                        // PlayerLeftWalk
                        SheetRegion::rect(0, 364, 36, 36),
                        SheetRegion::rect(60, 364, 36, 36),
                        SheetRegion::rect(120, 364, 36, 36),
                        SheetRegion::rect(180, 364, 36, 36),
                        SheetRegion::rect(240, 364, 36, 36),
                    ],
                    0.15,
                )
                .looped()
                .flip_horizontal(),
                // PlayerLeftJumpRise
                Animation::with_frame(SheetRegion::rect(60, 364, 36, 36)).flip_horizontal(),
                // PlayerLeftJumpFall
                Animation::with_frame(SheetRegion::rect(180, 364, 36, 36)).flip_horizontal(),
                Animation::with_frames(
                    &[
                        // PlayerLeftAttack
                        SheetRegion::rect(120, 364, 36, 36),
                    ],
                    0.07,
                )
                .flip_horizontal(),
                .looped()
                .flip_horizontal(),
            ],
        };
        game.enter_level(player_start);
        game
    }
    fn level(&self) -> &Level {
        &self.levels[self.current_level]
    }
    fn enter_level(&mut self, player_pos: Vec2) {
        self.doors.clear();
        self.enemies.clear();
        // we will probably enter at a door
        self.player.touching_door = true;
        self.player.pos = player_pos;
        for (etype, pos) in self.levels[self.current_level].starts().iter() {
            match etype {
                EntityType::Player => {}
                EntityType::Door(rm, x, y) => self.doors.push((rm.clone(), (*x, *y), *pos)),
            }
        }
    }
    fn sprite_count(&self) -> usize {
        //todo!("count how many entities and other sprites we have");
        self.level().sprite_count() + self.enemies.len() + 1
    }
    fn render(&mut self, frend: &mut Renderer) {
        // make this exactly as big as we need
        frend.sprite_group_resize(0, self.sprite_count());
        frend.sprite_group_set_camera(0, self.camera);

        let sprites_used = self.level().render_into(frend, 0);
        let (sprite_posns, sprite_gfx) = frend.sprites_mut(0, sprites_used..);

        let sprite_posns = &mut sprite_posns[self.enemies.len()..];
        let sprite_gfx = &mut sprite_gfx[self.enemies.len()..];
        sprite_posns[0] = self.player.trf();
        sprite_gfx[0] = self.player.anim.sample(&self.animations);

        frend.sprite_group_set_camera(
            0,
            Camera2D {
                screen_pos: [(self.player.pos.x - (W / 4) as f32), 7_f32],
                screen_size: [W as f32, H as f32],
            },
        );
    }
    fn simulate(&mut self, input: &Input, dt: f32) {
        let acc = Vec2 {
            // x: WALK_ACC * input.key_axis(Key::ArrowLeft, Key::ArrowRight),
            x: WALK_ACC,
            y: GRAV_ACC,
        };

        if input.is_key_down(Key::ArrowRight) {
            self.player.dir = Dir::E;
            self.player.anim.play(AnimationKey::PlayerRightWalk, false);
        } else if input.is_key_down(Key::ArrowLeft) {
            self.player.dir = Dir::W;
            self.player.anim.play(AnimationKey::PlayerLeftWalk, false);
        }

        // Handle jumping NOT WORKING
        if input.is_key_down(Key::ArrowUp) && self.player.grounded {
            self.player.jumping = true;
            self.player.jump_timer = JUMP_TIME_MAX;
            self.player.vel.y = JUMP_VEL;
        }

        if self.player.jumping {
            self.player.jump_timer -= dt;
            self.player.jumping = self.player.jump_timer > 0.0;
        }

        self.player.vel.y -= GRAV_ACC * dt;
        self.player.vel.x += acc.x * dt;
        if acc.x.abs() < 0.1 {
            self.player.vel.x *= BRAKE_DAMP;
        }
        self.player.vel.x = self.player.vel.x.clamp(-MAX_SPEED, MAX_SPEED);

        self.player.pos += self.player.vel * dt;

        let lw = self.level().width();
        let lh = self.level().height();

        self.player.pos.x = self.player.pos.x.clamp(
            0.0,
            lw as f32 * TILE_SZ as f32 - self.player.rect().w as f32 / 2.0,
        );
        self.player.pos.y = self
            .player
            .pos
            .y
            .clamp(0.0, H as f32 - self.player.rect().h as f32);

        // if player's y position is less than or equal to 0, then they are grounded
        // so, set grounded to True. If their y position is greater than 0, then they
        // are not grounded, so set grounded to False.
        self.player.grounded = self.player.pos.y <= 0.0;
        if self.player.grounded {
            // if the player is grounded, set their y velocity to 0
            self.player.vel.y = 0.0;
        }

        // if the player is jumping
        if self.player.jumping {
            if self.player.vel.y > 0.0 {
                self.player
                    .anim
                    .play(AnimationKey::PlayerRightJumpRise, false);
            } else {
                self.player
                    .anim
                    .play(AnimationKey::PlayerRightJumpFall, false);
            }
        } else {
            match self.player.dir {
                Dir::E => self.player.anim.play(AnimationKey::PlayerRightWalk, false),
                Dir::W => self.player.anim.play(AnimationKey::PlayerLeftWalk, false),
            }
        }

        self.player.anim.tick(dt);
    }
        }

        // Door collision and response
        let mut triggers = vec![];
        let prect = self.player.rect();

        let mut player_tile_contacts = vec![];

        self.player.grounded = false;
        for contact in player_tile_contacts {
            let disp = Self::compute_disp(self.player.rect(), contact.b_rect);
            self.player.pos += disp;
            if disp.y > 0.0 {
                self.player.grounded = true;
                self.player.vel.y = 0.0;
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
        self::Game::gather_contacts(&[prect], &door_rects, &mut triggers);
        if triggers.is_empty() {
            self.player.touching_door = false;
        }
        for Contact { b_index: door, .. } in triggers.drain(..) {
            // enter door if player has moved, wasn't previously touching door
            if !self.player.touching_door {
                self.player.touching_door = true;
                let (door_to, door_to_pos, _door_pos) = &self.doors[door];
                let dest = self
                    .levels
                    .iter()
                    .position(|l| l.name() == door_to)
                    .expect("door to invalid room {door_to}!");
                if dest == self.current_level {
                    self.player.pos = self
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
                    );
                }
                break;
            }
        }
        while self.player.pos.x
            > self.camera.screen_pos[0] + self.camera.screen_size[0] - SCREEN_FAST_MARGIN
        {
            self.camera.screen_pos[0] += 1.0;
        }
        while self.player.pos.x < self.camera.screen_pos[0] + SCREEN_FAST_MARGIN {
            self.camera.screen_pos[0] -= 1.0;
        }
        while self.player.pos.y
            > self.camera.screen_pos[1] + self.camera.screen_size[1] - SCREEN_FAST_MARGIN
        {
            self.camera.screen_pos[1] += 1.0;
        }
        while self.player.pos.y < self.camera.screen_pos[1] + SCREEN_FAST_MARGIN {
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
