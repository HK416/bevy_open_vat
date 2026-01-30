#import bevy_pbr::mesh_functions;
#import bevy_pbr::mesh_functions::get_world_from_local;
#import bevy_pbr::mesh_functions::mesh_position_local_to_world;
#import bevy_pbr::mesh_functions::mesh_normal_local_to_world;
#import bevy_pbr::view_transformations::position_world_to_clip
#import bevy_pbr::forward_io::VertexOutput;
#import bevy_pbr::mesh_view_bindings::globals;

// --- Structures ---

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
}

struct OpenVatParams {
    min_pos: vec3<f32>,
    frame_count: u32,
    max_pos: vec3<f32>,
    y_resolution: f32,
};

struct VatInstanceData {
    start_frame: u32,
    frame_count: u32,
    rate: f32,
    offset: f32,
};

// --- Bindings ---

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var vat_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var vat_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> ext: OpenVatParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<storage, read> instance_data: array<VatInstanceData>;

// --- Utility Functions ---

fn get_vat_data_safe(tag: u32) -> VatInstanceData {
    let safe_tag = tag % arrayLength(&instance_data);
    return instance_data[safe_tag];
}

fn apply_vat(frame_index: f32, v_pos: vec3<f32>, uv_vat: vec2<f32>) -> mat2x3<f32> {
    let frame_cnt = f32(ext.frame_count);
    let safe_frame = frame_index % frame_cnt;

    let current_frame = floor(safe_frame);
    let next_frame = (current_frame + 1.0) % frame_cnt;
    let blend = fract(safe_frame);

    let frame_step = 1.0 / ext.y_resolution;
    let uv_curr = uv_vat + vec2<f32>(0.0, current_frame * frame_step);
    let uv_next = uv_vat + vec2<f32>(0.0, next_frame * frame_step);

    let pos_curr = textureSampleLevel(vat_texture, vat_sampler, uv_curr, 0).rgb;
    let pos_next = textureSampleLevel(vat_texture, vat_sampler, uv_next, 0).rgb;
    let pos_mixed = mix(pos_curr, pos_next, blend);

    let range = ext.max_pos - ext.min_pos;
    let obj_pos = ext.min_pos + pos_mixed * range;

    // [Coordinate System Conversion]
    // Blender (Z-up Right-handed) -> Bevy (Y-up Right-handed)
    // Blender: Forward: -Y, Up: +Z, Right: +X
    // Bevy:    Forward: -Z, Up: +Y, Right: +X
    //
    // Mapping Logic:
    // Bevy.x = Blender.x
    // Bevy.y = Blender.z
    // Bevy.z = -Blender.y
    // 
    let final_pos = vec3<f32>(
        obj_pos.x,
        ext.min_pos.z + pos_mixed.z * range.z,
        -(ext.min_pos.y + pos_mixed.y * range.y)
    );

    let norm_curr_tex = textureSampleLevel(vat_texture, vat_sampler, uv_curr + vec2<f32>(0.0, 0.5), 0).rgb;
    let norm_next_tex = textureSampleLevel(vat_texture, vat_sampler, uv_next + vec2<f32>(0.0, 0.5), 0).rgb;

    var n_curr = norm_curr_tex * 2.0 - 1.0;
    var n_next = norm_next_tex * 2.0 - 1.0;
    
    // Apply the same coordinate conversion to normal vectors
    // (Swap Y <-> Z and invert Y for the new Z)
    n_curr = vec3<f32>(n_curr.x, n_curr.z, -n_curr.y);
    n_next = vec3<f32>(n_next.x, n_next.z, -n_next.y);

    let final_norm = normalize(mix(n_curr, n_next, blend));

    return mat2x3<f32>(v_pos + final_pos, final_norm);
}

// --- Vertex Shader ---

@vertex
fn main(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let tag = mesh_functions::get_tag(vertex.instance_index);
    let my_data = get_vat_data_safe(tag);
    
    let raw_progress = globals.time * my_data.rate + my_data.offset;
    let progress = fract(raw_progress);

    let relative_frame = progress * f32(my_data.frame_count);
    let absolute_frame = f32(my_data.start_frame) + relative_frame;

    let vat_result = apply_vat(absolute_frame, vertex.position, vertex.uv_b);
    let new_position = vat_result[0];
    let new_normal = vat_result[1];

    let world_from_local = get_world_from_local(vertex.instance_index);

    // Local -> World (Position)
    out.world_position = mesh_position_local_to_world(world_from_local, vec4<f32>(new_position, 1.0));
    
    // Local -> World (Normal)
    out.world_normal = mesh_normal_local_to_world(new_normal, vertex.instance_index);
    
    // World -> Clip
    out.position = position_world_to_clip(out.world_position.xyz);
    
    out.uv = vertex.uv;
    
    return out;
}
