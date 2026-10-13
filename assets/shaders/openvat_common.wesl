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
        return VatInstanceData(0u, 1u, 0.0, 0.0, 1u);
    }
    let safe_tag = tag % len;
    return instance_data[safe_tag];
}

// Absolute (fractional) VAT frame for an instance at the given shader time.
fn vat_frame(data: VatInstanceData, time: f32) -> f32 {
    let progress = time * data.rate + data.offset;
    if data.looping != 0u {
        return f32(data.start_frame) + fract(progress) * f32(data.frame_count);
    }
    // Hold the first or last frame instead of wrapping around.
    let frame = f32(data.start_frame) + saturate(progress) * f32(data.frame_count);
    return min(frame, f32(ext.frame_count - 1u));
}

// The texels to read for a fractional frame, and how far to blend between them.
struct VatSample {
    column: i32,
    row_curr: i32,
    row_next: i32,
    blend: f32,
};

fn vat_sample(frame_index: f32, uv_vat: vec2<f32>) -> VatSample {
    let frame_cnt = f32(ext.frame_count);
    let safe_frame = frame_index % frame_cnt;
    let current_frame = floor(safe_frame);
    let next_frame = (current_frame + 1.0) % frame_cnt;

    // `uv_vat` points at this vertex's texel in frame 0; each frame is one row further down.
    let base = vec2<i32>(uv_vat * vec2<f32>(textureDimensions(vat_texture)));
    return VatSample(base.x, base.y + i32(current_frame), base.y + i32(next_frame), fract(safe_frame));
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
