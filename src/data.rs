use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
    render::render_resource::ShaderType,
};

use crate::asset::{RemapInfo, VatAnimationClip};

/// Internal marker component indicating the entity's material has been swapped
/// and the shader buffer is ready.
#[derive(Component)]
pub struct VatMaterialReady;

/// Marker component to indicate that VatAnimator has been propagated to children.
/// Unlike VatMaterialReady, this is for entities without StandardMaterial (container nodes).
#[derive(Component)]
pub struct VatPropagated;

/// Links a mesh entity's `VatAnimator` copy to the animator on the entity it was propagated from.
///
/// Changes to the source animator are copied to every linked mesh automatically, so control
/// playback on the source (for example the glTF scene root), not on the linked meshes.
#[derive(Component, Debug)]
#[relationship(relationship_target = VatAnimatedMeshes)]
pub struct VatAnimatorOf(pub Entity);

/// The mesh entities that receive copies of this entity's `VatAnimator`.
#[derive(Component, Debug)]
#[relationship_target(relationship = VatAnimatorOf)]
pub struct VatAnimatedMeshes(Vec<Entity>);

/// Triggered on the entity that owns a `VatAnimator` when a non-looping clip reaches its end.
///
/// For propagated animators it is only triggered on the source entity, not on the linked meshes.
#[derive(EntityEvent, Debug, Clone)]
pub struct VatClipFinished {
    /// The entity whose clip finished.
    pub entity: Entity,
    /// The clip that finished.
    pub clip: Handle<VatAnimationClip>,
}

/// Component to control the playback of a VAT animation on an entity.
/// Add this to an entity to automatically convert its material to a VAT material.
///
/// Playback state is stored as an anchor (a clip position at a point in time) so the GPU can
/// advance the animation on its own. Use the methods below to change playback; they re-anchor
/// at the current time so the animation continues smoothly from where it is.
#[derive(Debug, Clone, Component, Reflect)]
#[require(Transform, Visibility)]
#[component(on_insert = anchor_on_insert)]
pub struct VatAnimator {
    /// Handle to the remapping info asset.
    pub remap_info: Handle<RemapInfo>,
    /// Handle to the VAT image texture.
    pub vat_texture: Handle<Image>,
    current_clip: Handle<VatAnimationClip>,
    speed: f32,
    is_playing: bool,
    /// Clip playback position in seconds at `anchor_time`.
    anchor_elapsed: f32,
    /// Elapsed app time in seconds that the anchor refers to; set when the component is added.
    anchor_time: Option<f64>,
}

impl Default for VatAnimator {
    fn default() -> Self {
        Self {
            remap_info: Default::default(),
            vat_texture: Default::default(),
            current_clip: Default::default(),
            speed: 1.0,
            is_playing: true,
            anchor_elapsed: 0.0,
            anchor_time: None,
        }
    }
}

/// Anchors a newly inserted animator at the current time, including one that replaces an existing
/// animator. Propagated copies keep their source's anchor.
fn anchor_on_insert(mut world: DeferredWorld, context: HookContext) {
    let Some(now) = world.get_resource::<Time>().map(Time::elapsed_secs_f64) else {
        return;
    };
    if let Some(mut animator) = world.get_mut::<VatAnimator>(context.entity)
        && animator.anchor_time.is_none()
    {
        animator.anchor_time = Some(now);
    }
}

impl VatAnimator {
    /// Creates an animator that starts playing `clip` at normal speed.
    pub fn new(
        remap_info: Handle<RemapInfo>,
        vat_texture: Handle<Image>,
        clip: Handle<VatAnimationClip>,
    ) -> Self {
        Self {
            remap_info,
            vat_texture,
            current_clip: clip,
            ..default()
        }
    }

    /// Sets the playback speed multiplier (1.0 is normal speed, negative plays backwards).
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Starts playback this many seconds into the clip, e.g. to desynchronize a crowd.
    pub fn with_elapsed(mut self, seconds: f32) -> Self {
        self.anchor_elapsed = seconds;
        self
    }

    /// Starts in the paused state.
    pub fn paused(mut self) -> Self {
        self.is_playing = false;
        self
    }

    /// The clip currently being played.
    pub fn current_clip(&self) -> &Handle<VatAnimationClip> {
        &self.current_clip
    }

    /// The playback speed multiplier.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Whether the animation is currently playing.
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Clip playback position in seconds. Not wrapped: looping clips keep counting up.
    pub fn elapsed(&self, time: &Time) -> f32 {
        self.elapsed_at(time.elapsed_secs_f64())
    }

    /// Switches to `clip` and plays it from the start.
    pub fn play(&mut self, clip: Handle<VatAnimationClip>, time: &Time) {
        self.current_clip = clip;
        self.is_playing = true;
        self.seek(0.0, time);
    }

    /// Pauses playback at the current position.
    pub fn pause(&mut self, time: &Time) {
        self.rebase(time.elapsed_secs_f64());
        self.is_playing = false;
    }

    /// Resumes playback from the current position.
    pub fn resume(&mut self, time: &Time) {
        self.rebase(time.elapsed_secs_f64());
        self.is_playing = true;
    }

    /// Changes the playback speed without jumping to a different position.
    pub fn set_speed(&mut self, speed: f32, time: &Time) {
        self.rebase(time.elapsed_secs_f64());
        self.speed = speed;
    }

    /// Jumps to `seconds` into the clip.
    pub fn seek(&mut self, seconds: f32, time: &Time) {
        self.anchor_elapsed = seconds;
        self.anchor_time = Some(time.elapsed_secs_f64());
    }

    fn elapsed_at(&self, now: f64) -> f32 {
        let Some(anchor_time) = self.anchor_time else {
            return self.anchor_elapsed;
        };
        let speed = if self.is_playing { self.speed } else { 0.0 };
        self.anchor_elapsed + ((now - anchor_time) * speed as f64) as f32
    }

    fn rebase(&mut self, now: f64) {
        self.anchor_elapsed = self.elapsed_at(now);
        self.anchor_time = Some(now);
    }

    /// Builds the GPU data for this animator.
    ///
    /// The shader computes `globals.time * rate + offset`, where `globals.time` is the elapsed time
    /// wrapped by `Time::wrap_period`. `wrap_base` is the unwrapped minus the wrapped elapsed time;
    /// folding it into `offset` keeps playback continuous when the shader's time wraps.
    pub fn instance_data(&self, clip: &VatAnimationClip, wrap_base: f64) -> VatInstanceData {
        let duration = clip.duration().filter(|d| *d > 0.0).unwrap_or(1.0) as f64;
        let speed = if self.is_playing { self.speed as f64 } else { 0.0 };
        let anchor_time = self.anchor_time.unwrap_or(wrap_base);

        let mut offset = (self.anchor_elapsed as f64 + (wrap_base - anchor_time) * speed) / duration;
        if clip.looping {
            // Only the fractional cycle matters for a loop, and dropping the whole cycles keeps
            // the f32 offset precise however long the app has been running.
            offset = offset.rem_euclid(1.0);
        }

        VatInstanceData {
            start_frame: clip.start_frame,
            frame_count: clip.frame_count(),
            rate: (speed / duration) as f32,
            offset: offset as f32,
            looping: clip.looping as u32,
        }
    }

    /// Elapsed app time in seconds at which a non-looping clip reaches its end
    /// (or its start, when playing backwards). `None` if it never finishes, or if it is
    /// already there, so changing an animator after its clip finished does not finish it again.
    pub fn finish_time(&self, clip: &VatAnimationClip) -> Option<f64> {
        if clip.looping || !self.is_playing || self.speed == 0.0 {
            return None;
        }
        let duration = clip.duration()? as f64;
        let anchor_time = self.anchor_time?;
        let target = if self.speed > 0.0 { duration } else { 0.0 };
        let remaining = (target - self.anchor_elapsed as f64) / self.speed as f64;
        (remaining > 0.0).then_some(anchor_time + remaining)
    }
}

/// Data sent to the GPU for each instance, containing the current animation state.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Reflect, ShaderType, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VatInstanceData {
    /// The starting frame of the animation clip.
    pub start_frame: u32,
    /// The total number of frames in the clip.
    pub frame_count: u32,
    /// The playback rate in clip cycles per second (speed / duration).
    pub rate: f32,
    /// The clip progress at shader time zero, in clip cycles.
    pub offset: f32,
    /// Non-zero if the clip loops; otherwise playback holds the first or last frame.
    pub looping: u32,
}

impl Default for VatInstanceData {
    /// No clip: the shader shows `start_frame` (frame 0) without animating.
    fn default() -> Self {
        Self {
            start_frame: 0,
            frame_count: 0,
            rate: 0.0,
            offset: 0.0,
            looping: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    fn clip(looping: bool) -> VatAnimationClip {
        // 20 frames at 10 fps = 2 seconds
        VatAnimationClip {
            start_frame: 5,
            end_frame: 25,
            frame_rate: 10.0,
            looping,
        }
    }

    fn time_at(seconds: f64) -> Time {
        let mut time = Time::<()>::default();
        time.advance_to(Duration::from_secs_f64(seconds));
        time
    }

    fn cycle(progress: f32) -> f32 {
        progress.rem_euclid(1.0)
    }

    /// What the shader computes before looping/clamping, in clip cycles.
    fn gpu_progress(animator: &VatAnimator, clip: &VatAnimationClip, time: &Time) -> f32 {
        let wrap_base = time.elapsed_secs_f64() - time.elapsed_secs_wrapped_f64();
        let data = animator.instance_data(clip, wrap_base);
        time.elapsed_secs_wrapped() * data.rate + data.offset
    }

    fn animator_at(seconds: f64) -> VatAnimator {
        VatAnimator {
            anchor_time: Some(seconds),
            ..default()
        }
    }

    #[test]
    fn test_elapsed_advances_with_speed() {
        let mut animator = animator_at(10.0);
        assert_eq!(animator.elapsed(&time_at(11.5)), 1.5);

        animator.set_speed(2.0, &time_at(11.5));
        assert_eq!(animator.elapsed(&time_at(11.5)), 1.5, "speed change must not jump");
        assert_eq!(animator.elapsed(&time_at(12.0)), 2.5);
    }

    #[test]
    fn test_pause_holds_current_position() {
        let mut animator = animator_at(0.0);
        animator.pause(&time_at(3.0));
        assert_eq!(animator.elapsed(&time_at(10.0)), 3.0);

        animator.resume(&time_at(10.0));
        assert_eq!(animator.elapsed(&time_at(11.0)), 4.0);
    }

    #[test]
    fn test_seek_and_play() {
        let mut animator = animator_at(0.0).with_speed(0.5);
        animator.seek(1.25, &time_at(4.0));
        assert_eq!(animator.elapsed(&time_at(6.0)), 2.25);

        animator.pause(&time_at(6.0));
        animator.play(Handle::default(), &time_at(8.0));
        assert!(animator.is_playing());
        assert_eq!(animator.elapsed(&time_at(10.0)), 1.0, "play restarts at the clip start");
    }

    #[test]
    fn test_gpu_progress_matches_elapsed() {
        let clip = clip(true);
        let animator = animator_at(1.0).with_speed(1.5).with_elapsed(0.3);
        for seconds in [1.0, 2.7, 9.0] {
            let time = time_at(seconds);
            let expected = animator.elapsed(&time) / clip.duration().unwrap();
            assert!((cycle(gpu_progress(&animator, &clip, &time)) - cycle(expected)).abs() < 1e-4);

            let once = VatAnimationClip { looping: false, ..clip };
            assert!((gpu_progress(&animator, &once, &time) - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn test_gpu_progress_survives_time_wrap() {
        let clip = clip(true);
        let animator = animator_at(5.0);

        let mut before = time_at(0.0);
        before.set_wrap_period(Duration::from_secs(10));
        before.advance_to(Duration::from_secs_f64(9.5));
        let mut after = before;
        after.advance_to(Duration::from_secs_f64(10.5));

        // Shader time jumps from 9.5 to 0.5, but progress must still advance by 1 second.
        let delta = gpu_progress(&animator, &clip, &after) - gpu_progress(&animator, &clip, &before);
        assert!((cycle(delta) - 1.0 / clip.duration().unwrap()).abs() < 1e-4);
    }

    #[test]
    fn test_looping_offset_stays_small_after_long_uptime() {
        let clip = clip(true);
        let animator = animator_at(0.0);
        // A day in, with the default one-hour wrap period.
        let data = animator.instance_data(&clip, 24.0 * 3600.0);
        assert!((0.0..1.0).contains(&data.offset), "offset {} lost f32 precision", data.offset);
    }

    #[test]
    fn test_reinserted_animator_is_anchored() {
        let mut world = World::new();
        world.insert_resource(time_at(3.0));
        let entity = world.spawn(VatAnimator::default()).id();
        assert_eq!(world.get::<VatAnimator>(entity).unwrap().anchor_time, Some(3.0));

        world.insert_resource(time_at(7.0));
        world.entity_mut(entity).insert(VatAnimator::default());
        assert_eq!(world.get::<VatAnimator>(entity).unwrap().anchor_time, Some(7.0));

        // Copies propagated to child meshes keep their source's anchor.
        let copy = world.get::<VatAnimator>(entity).unwrap().clone();
        world.insert_resource(time_at(9.0));
        let child = world.spawn(copy).id();
        assert_eq!(world.get::<VatAnimator>(child).unwrap().anchor_time, Some(7.0));
    }

    #[test]
    fn test_finish_time() {
        let animator = animator_at(10.0).with_elapsed(0.5);
        assert_eq!(animator.finish_time(&clip(false)), Some(11.5));
        assert_eq!(animator.finish_time(&clip(true)), None, "looping clips never finish");
        assert_eq!(animator.clone().paused().finish_time(&clip(false)), None);

        let backwards = animator_at(10.0).with_elapsed(0.5).with_speed(-0.5);
        assert_eq!(backwards.finish_time(&clip(false)), Some(11.0));

        let past_end = animator_at(10.0).with_elapsed(5.0);
        assert_eq!(past_end.finish_time(&clip(false)), None, "already finished: no second event");
    }

    #[test]
    fn test_instance_data_looping_flag() {
        let animator = animator_at(0.0);
        assert_eq!(animator.instance_data(&clip(true), 0.0).looping, 1);
        assert_eq!(animator.instance_data(&clip(false), 0.0).looping, 0);
    }
}
