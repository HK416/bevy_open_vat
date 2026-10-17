//! Headless render tests.
//!
//! Each test renders to offscreen images with a fixed time step, captures frames with Bevy's
//! `Screenshot`, writes them to `target/render_tests/*.png` for inspection, and compares them
//! numerically. No reference images are stored: the control is always rendered in the same run.
//!
//! * `vat_matches_standard_material_at_zero_deformation`: the same mesh drawn with a plain
//!   `StandardMaterial` (control) and with a VAT texture that encodes zero offsets and the mesh's
//!   own normals (VAT), by two cameras in the same frame. The two images must match, which pins
//!   everything the VAT vertex shader has to forward unchanged: instance index (mesh flags,
//!   shadows), tangents (normal maps), vertex colors and motion vectors (TAA).
//! * `fox_animates_on_gpu_and_pauses`: real OpenVAT assets; frames differ while playing and are
//!   identical while paused.
//!
//! A GPU adapter is required (Metal on macOS, Vulkan/lavapipe on Linux).

use std::{
    path::PathBuf,
    sync::{Mutex, Once},
    time::Duration,
};

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    app::PluginsState,
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::prepass::NormalPrepass,
    log::{
        LogPlugin,
        tracing::{Event, Level, Subscriber, field::Visit},
        tracing_subscriber::{Layer, layer::Context},
    },
    mesh::VertexAttributeValues,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};

use crate::{material::VatStandardMaterial, plugin::OpenVatPlugin, prelude::*};

const SIZE: u32 = 256;
const FRAME: Duration = Duration::from_micros(16_667);
/// Frames rendered before a capture is taken, so TAA history has converged.
const SETTLE_FRAMES: usize = 12;

// ---------- error capture ----------

static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Tests share the global logger and `ERRORS`, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());
static LOGGER: Once = Once::new();

struct ErrorCapture;

impl<S: Subscriber> Layer<S> for ErrorCapture {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR {
            return;
        }
        struct Message(String);
        impl Visit for Message {
            fn record_debug(
                &mut self,
                _: &bevy::log::tracing::field::Field,
                v: &dyn std::fmt::Debug,
            ) {
                self.0.push_str(&format!("{v:?} "));
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        ERRORS.lock().unwrap().push(message.0);
    }
}

fn assert_no_errors() {
    let errors = ERRORS.lock().unwrap().clone();
    assert!(
        errors.is_empty(),
        "errors logged while rendering: {errors:#?}"
    );
}

// ---------- app / scene helpers ----------

#[derive(Resource, Default)]
struct Captures(Vec<Image>);

/// Returns `None` (and prints a note) when no GPU adapter is available, so the tests skip
/// instead of failing on a headless machine. Any other setup panic still propagates.
fn try_headless_app() -> Option<App> {
    match std::panic::catch_unwind(headless_app) {
        Ok(app) => Some(app),
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("");
            if message.contains("Unable to find a GPU") {
                eprintln!("skipping render test: no GPU adapter");
                None
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn headless_app() -> App {
    ERRORS.lock().unwrap().clear();
    let mut app = App::new();
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .disable::<WinitPlugin>();
    // The global tracing subscriber can only be installed once per process; later apps reuse it.
    let mut first = false;
    LOGGER.call_once(|| first = true);
    plugins = if first {
        plugins.set(LogPlugin {
            custom_layer: |_| Some(Box::new(ErrorCapture)),
            ..default()
        })
    } else {
        plugins.disable::<LogPlugin>()
    };
    app.add_plugins((plugins, OpenVatPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Captures>();
    // `App::run` would do this; the renderer is created asynchronously in `finish`.
    while app.plugins_state() != PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Camera rendering into an offscreen image, optionally with TAA (depth + motion vector prepass)
/// and a normal prepass. Returns the render target.
fn spawn_camera(
    world: &mut World,
    eye: Vec3,
    look_at: Vec3,
    taa: bool,
    layers: RenderLayers,
) -> Handle<Image> {
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            SIZE,
            SIZE,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    let mut camera = world.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.clone().into()),
        Msaa::Off,
        Transform::from_translation(eye).looking_at(look_at, Vec3::Y),
        layers,
    ));
    if taa {
        camera.insert((TemporalAntiAliasing::default(), NormalPrepass));
    }
    target
}

fn spawn_light(world: &mut World, layers: RenderLayers) {
    world.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 10_000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 8.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
        layers,
    ));
}

fn run_frames(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// Runs `app.update()` until `done` returns true, failing after `max_frames`.
fn run_until(app: &mut App, max_frames: usize, mut done: impl FnMut(&mut World) -> bool) {
    for _ in 0..max_frames {
        app.update();
        if done(app.world_mut()) {
            return;
        }
    }
    panic!("condition not met within {max_frames} frames");
}

fn output_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/render_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.png"))
}

/// Captures every target in the same frame, saves each as `target/render_tests/<name>.png`, and
/// returns the images in the given order.
fn capture_all(app: &mut App, targets: &[(&Handle<Image>, &str)]) -> Vec<Image> {
    let before = app.world().resource::<Captures>().0.len();
    for (target, name) in targets {
        app.world_mut()
            .spawn(Screenshot::image((*target).clone()))
            .observe(save_to_disk(output_path(name)))
            .observe(
                |shot: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                    captures.0.push(shot.image.clone());
                },
            );
    }
    run_until(app, 20, |world| {
        world.resource::<Captures>().0.len() >= before + targets.len()
    });
    app.world().resource::<Captures>().0[before..].to_vec()
}

fn capture(app: &mut App, target: &Handle<Image>, name: &str) -> Image {
    capture_all(app, &[(target, name)]).remove(0)
}

// ---------- image comparison ----------

struct Diff {
    /// Mean absolute RGB difference on a 0..255 scale.
    mean: f64,
    /// Fraction of pixels whose largest channel difference exceeds 32.
    bad_fraction: f64,
}

fn diff(a: &Image, b: &Image) -> Diff {
    let (a, b) = (a.data.as_ref().unwrap(), b.data.as_ref().unwrap());
    assert_eq!(a.len(), b.len(), "captures differ in size");
    let (mut sum, mut bad) = (0u64, 0usize);
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let channel = |i: usize| (pa[i] as i32 - pb[i] as i32).unsigned_abs();
        let d = [channel(0), channel(1), channel(2)];
        sum += d.iter().map(|&c| c as u64).sum::<u64>();
        if d.iter().any(|&c| c > 32) {
            bad += 1;
        }
    }
    let pixels = a.len() / 4;
    Diff {
        mean: sum as f64 / (pixels as f64 * 3.0),
        bad_fraction: bad as f64 / pixels as f64,
    }
}

/// Number of pixels that differ from the clear color.
fn drawn_pixels(image: &Image, clear: [u8; 3]) -> usize {
    image
        .data
        .as_ref()
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3] != clear)
        .count()
}

// ---------- zero-deformation VAT assets ----------

/// A VAT texture that leaves `mesh` exactly where it is: every position texel decodes to a zero
/// offset (with a `[-1, 1]` remap) and every normal texel is the mesh's own normal, stored in
/// Blender's Z-up space as OpenVAT does. Layout: one column per vertex, `frames` rows of
/// positions followed by `frames` rows of normals.
fn identity_vat_texture(mesh: &Mesh, frames: u32) -> Image {
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("mesh has no normals");
    };
    let width = normals.len() as u32;
    let mut data = Vec::with_capacity((width * frames * 2 * 16) as usize);
    for _ in 0..frames {
        for _ in 0..width {
            data.extend([0.5f32, 0.5, 0.5, 1.0].iter().flat_map(|f| f.to_le_bytes()));
        }
    }
    for _ in 0..frames {
        for [x, y, z] in normals {
            // Bevy (x, y, z) -> Blender (x, -z, y), then [-1, 1] -> [0, 1].
            let n = Vec3::new(*x, -*z, *y) * 0.5 + 0.5;
            data.extend([n.x, n.y, n.z, 1.0].iter().flat_map(|f| f.to_le_bytes()));
        }
    }
    Image::new(
        Extent3d {
            width,
            height: frames * 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba32Float,
        default(),
    )
}

/// UV1 pointing each vertex at its own column in frame 0.
fn vat_uvs(vertex_count: usize, texture_height: u32) -> VertexAttributeValues {
    let v = 0.5 / texture_height as f32;
    VertexAttributeValues::Float32x2(
        (0..vertex_count)
            .map(|i| [(i as f32 + 0.5) / vertex_count as f32, v])
            .collect(),
    )
}

/// A cuboid with tangents, per-vertex colors and VAT UVs, so the `VERTEX_TANGENTS` and
/// `VERTEX_COLORS` shader paths are exercised.
fn test_mesh(texture_height: u32) -> Mesh {
    let mut mesh = Cuboid::default().mesh().build();
    let count = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vat_uvs(count, texture_height));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(
            (0..count)
                .map(|i| {
                    [
                        (i % 3) as f32 / 2.0,
                        (i % 5) as f32 / 4.0,
                        (i % 7) as f32 / 6.0,
                        1.0,
                    ]
                })
                .collect(),
        ),
    );
    mesh.generate_tangents().unwrap();
    mesh
}

/// 8×8 linear normal map alternating flat and tilted normals, so tangent handling shows.
fn normal_map() -> Image {
    let mut data = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let n = if (x + y) % 2 == 0 {
                Vec3::new(0.6, 0.0, 0.8)
            } else {
                Vec3::Z
            };
            let n = (n * 0.5 + 0.5) * 255.0;
            data.extend([n.x as u8, n.y as u8, n.z as u8, 255]);
        }
    }
    Image::new(
        Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        default(),
    )
}

#[derive(Component)]
struct Spinning;

fn spin(mut query: Query<&mut Transform, With<Spinning>>) {
    for mut transform in &mut query {
        transform.rotate_y(0.02);
    }
}

// ---------- tests ----------

#[test]
fn vat_matches_standard_material_at_zero_deformation() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut app) = try_headless_app() else {
        return;
    };
    app.add_systems(Update, spin);
    let world = app.world_mut();

    // Two cameras render the same frame: one sees the control cube (layer 1), the other the VAT
    // cube (layer 2). Shared props and the light are on both layers. Same frame means the same
    // TAA jitter, time and pose, so the images can be compared directly.
    let control_layer = RenderLayers::layer(1);
    let vat_layer = RenderLayers::layer(2);
    let both = control_layer.clone().with(2);
    let (eye, look_at) = (Vec3::new(2.0, 2.0, 3.0), Vec3::ZERO);
    let control_target = spawn_camera(world, eye, look_at, true, control_layer.clone());
    let vat_target = spawn_camera(world, eye, look_at, true, vat_layer.clone());
    spawn_light(world, both.clone());

    const FRAMES: u32 = 2;
    let mesh_asset = test_mesh(FRAMES * 2);
    let vat_texture = world
        .resource_mut::<Assets<Image>>()
        .add(identity_vat_texture(&mesh_asset, FRAMES));
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh_asset);
    let normal_map = world.resource_mut::<Assets<Image>>().add(normal_map());
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            normal_map_texture: Some(normal_map),
            ..default()
        });
    let remap = world.resource_mut::<Assets<RemapInfo>>().add(RemapInfo {
        os_remap: OsRemap {
            min: [-1.0; 3],
            max: [1.0; 3],
            frames: FRAMES,
        },
        animations: default(),
    });
    let clip = world
        .resource_mut::<Assets<VatAnimationClip>>()
        .add(VatAnimationClip {
            start_frame: 0,
            end_frame: 1,
            frame_rate: 1.0,
            looping: false,
        });

    // Ground receiving the cube's shadow, and a sphere casting a shadow onto the cube.
    let ground_mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(6.0, 6.0));
    let ground_material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(Color::srgb(0.8, 0.8, 0.8));
    world.spawn((
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_material.clone()),
        Transform::from_xyz(0.0, -0.5, 0.0),
        both.clone(),
    ));
    let sphere = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(0.3));
    world.spawn((
        Mesh3d(sphere),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(0.3, 1.6, 0.0),
        both,
    ));

    world.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Spinning,
        control_layer,
    ));
    let vat = world
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            VatAnimator::new(remap, vat_texture, clip),
            Spinning,
            vat_layer,
        ))
        .id();

    run_until(&mut app, 60, |world| {
        world
            .get::<MeshMaterial3d<VatStandardMaterial>>(vat)
            .is_some()
    });
    run_frames(&mut app, SETTLE_FRAMES);
    let frames = capture_all(
        &mut app,
        &[
            (&control_target, "zero_deformation_control"),
            (&vat_target, "zero_deformation_vat"),
        ],
    );

    assert_no_errors();
    let d = diff(&frames[0], &frames[1]);
    assert!(
        d.mean < 1.0 && d.bad_fraction < 0.002,
        "VAT render differs from StandardMaterial render: mean {:.3}/255, {:.2}% pixels off (see target/render_tests/zero_deformation_*.png)",
        d.mean,
        d.bad_fraction * 100.0
    );
}

#[test]
fn fox_animates_on_gpu_and_pauses() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut app) = try_headless_app() else {
        return;
    };
    let world = app.world_mut();
    // No TAA here: its per-frame jitter would make even a paused fox differ slightly.
    let target = spawn_camera(
        world,
        Vec3::new(0.0, 1.0, 2.2),
        Vec3::new(0.0, 0.4, 0.0),
        false,
        RenderLayers::default(),
    );
    spawn_light(world, RenderLayers::default());

    let assets = world.resource::<AssetServer>().clone();
    let remap: Handle<RemapInfo> = assets.load("models/vat/Fox-remap_info.json");
    let texture: Handle<Image> = assets.load("models/vat/Fox_vat.exr");
    let clip: Handle<VatAnimationClip> = assets.load("models/vat/Fox-remap_info.json#Run");
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset("models/vat/Fox.glb"));
    let fox = world
        .spawn((
            WorldAssetRoot(scene),
            Transform::from_scale(Vec3::splat(0.01)),
            VatAnimator::new(remap, texture.clone(), clip),
        ))
        .id();

    run_until(&mut app, 600, |world| {
        world
            .query_filtered::<Entity, With<MeshMaterial3d<VatStandardMaterial>>>()
            .iter(world)
            .count()
            > 0
    });

    run_frames(&mut app, SETTLE_FRAMES);
    let playing_a = capture(&mut app, &target, "fox_playing_a");
    run_frames(&mut app, 15);
    let playing_b = capture(&mut app, &target, "fox_playing_b");

    let world = app.world_mut();
    let now = *world.resource::<Time>();
    world.get_mut::<VatAnimator>(fox).unwrap().pause(&now);
    run_frames(&mut app, SETTLE_FRAMES);
    let paused_a = capture(&mut app, &target, "fox_paused_a");
    run_frames(&mut app, 15);
    let paused_b = capture(&mut app, &target, "fox_paused_b");

    assert_no_errors();
    let fox_texture = app
        .world()
        .resource::<Assets<Image>>()
        .get(&texture)
        .expect("fox texture loaded");
    assert_eq!(
        fox_texture.texture_descriptor.format,
        TextureFormat::Rgba16Unorm,
        "EXR must be compacted before the fox is drawn"
    );
    assert!(fox_texture.data.is_none(), "CPU copy must be dropped after upload");
    let clear = app
        .world()
        .resource::<ClearColor>()
        .0
        .to_srgba()
        .to_u8_array();
    let drawn = drawn_pixels(&playing_a, [clear[0], clear[1], clear[2]]);
    assert!(drawn > 500, "fox not drawn: {drawn} non-background pixels");

    let playing = diff(&playing_a, &playing_b);
    assert!(
        playing.mean > 0.5,
        "fox did not animate: mean diff {:.3}/255",
        playing.mean
    );
    let paused = diff(&paused_a, &paused_b);
    assert!(
        paused.mean < 0.2,
        "paused fox still moves: mean diff {:.3}/255",
        paused.mean
    );
}
