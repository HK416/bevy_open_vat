#import bevy_pbr::mesh_functions;
#import bevy_pbr::mesh_functions::get_world_from_local;
#import bevy_pbr::mesh_functions::mesh_position_local_to_world;
#import bevy_pbr::mesh_functions::mesh_normal_local_to_world;
#import bevy_pbr::view_transformations::position_world_to_clip
#import bevy_pbr::forward_io::VertexOutput;
#import bevy_pbr::mesh_view_bindings::globals;
#import bevy_open_vat::common::{apply_vat, get_vat_data_safe};

// --- Structures ---

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
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
