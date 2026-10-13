// --- Structures ---

struct OpenVatParams {
    min_pos: vec3<f32>,
    frame_count: u32,
    max_pos: vec3<f32>,
};

struct VatInstanceData {
    start_frame: u32,
    frame_count: u32,
    rate: f32,
    offset: f32,
    looping: u32,
};

// --- Bindings ---

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var vat_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> ext: OpenVatParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<storage, read> instance_data: array<VatInstanceData>;

// --- Utility Functions ---

fn get_vat_data_safe(tag: u32) -> VatInstanceData {
    let len = arrayLength(&instance_data);
    if len == 0u {
        return VatInstanceData(0u, 0u, 0.0, 0.0, 1u);
    }
    let safe_tag = tag % len;
    return instance_data[safe_tag];
}

// The texels to read for an instance at the given shader time, and how far to blend between them.
struct VatSample {
    column: i32,
    row_curr: i32,
    row_next: i32,
    blend: f32,
};

// OpenVAT clip ranges share their boundary frame with the neighbouring clip: frame `start` holds
// the previous clip's last pose, so a clip's own frames are `start + 1 ..= end`. Looping clips are
// authored so that `end` repeats their first pose, so a loop runs end -> start + 1 -> ... -> end.
fn vat_sample(data: VatInstanceData, time: f32, uv_vat: vec2<f32>) -> VatSample {
    let progress = time * data.rate + data.offset;
    let count = data.frame_count;
    var curr: u32;
    var next: u32;
    var blend: f32;
    if count == 0u {
        // No clip: show `start_frame` (frame 0 for the default data) without animating.
        curr = data.start_frame;
        next = curr;
        blend = 0.0;
    } else if data.looping != 0u {
        let local = fract(progress) * f32(count);
        let k = min(u32(local), count - 1u);
        // Pose 0 is the clip's first pose, stored at `end`.
        curr = select(data.start_frame + k, data.start_frame + count, k == 0u);
        next = data.start_frame + k + 1u;
        blend = fract(local);
    } else {
        // Play the clip's own frames once, then hold the last one (`end`).
        let local = saturate(progress) * f32(count - 1u);
        let k = min(u32(local), count - 1u);
        curr = data.start_frame + 1u + k;
        next = min(curr + 1u, data.start_frame + count);
        blend = local - f32(k);
    }

    // `uv_vat` points at this vertex's texel in frame 0; each frame is one row further down.
    let base = vec2<i32>(uv_vat * vec2<f32>(textureDimensions(vat_texture)));
    return VatSample(
        base.x,
        base.y + i32(curr % ext.frame_count),
        base.y + i32(next % ext.frame_count),
        blend,
    );
}

// Exact texel reads: no sampler, no filtering between neighboring vertices or frames.
fn vat_load(column: i32, row: i32) -> vec3<f32> {
    return textureLoad(vat_texture, vec2<i32>(column, row), 0).rgb;
}

// [Coordinate System Conversion]
// Blender (Z-up Right-handed) -> Bevy (Y-up Right-handed)
// Blender: Forward: -Y, Up: +Z, Right: +X
// Bevy:    Forward: -Z, Up: +Y, Right: +X
//
// Mapping Logic:
// Bevy.x = Blender.x
// Bevy.y = Blender.z
// Bevy.z = -Blender.y
fn blender_to_bevy(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(v.x, v.z, -v.y);
}

fn vat_position(s: VatSample, v_pos: vec3<f32>) -> vec3<f32> {
    let encoded = mix(vat_load(s.column, s.row_curr), vat_load(s.column, s.row_next), s.blend);
    let offset = ext.min_pos + encoded * (ext.max_pos - ext.min_pos);
    return v_pos + blender_to_bevy(offset);
}

// Only call this when the normal is used: it costs two more texture reads.
fn vat_normal(s: VatSample) -> vec3<f32> {
    // Normals are stored in the lower half of the texture.
    let half = i32(textureDimensions(vat_texture).y / 2u);
    let n_curr = vat_load(s.column, s.row_curr + half) * 2.0 - 1.0;
    let n_next = vat_load(s.column, s.row_next + half) * 2.0 - 1.0;
    return blender_to_bevy(normalize(mix(n_curr, n_next, s.blend)));
}
