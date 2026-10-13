import bevy_pbr::{
    render::{
        mesh_functions::{get_tag, get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
        view_transformations::position_world_to_clip,
    },
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
};
import bevy_render::globals::Globals;
import bevy_open_vat::openvat_common::{get_vat_data_safe, vat_frame, vat_normal, vat_position, vat_sample};

// Prepass uses a simpler view bind group than the main pass,
// so mesh_view_bindings::globals (binding 11) cannot be used.
// We must declare the binding directly.
@group(0) @binding(1) var<uniform> globals: Globals;

// --- Vertex Shader ---

@vertex
fn main(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Handles meshes with compressed vertex attributes (`Mesh::compress_*`)
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);

    let tag = get_tag(vertex.instance_index);
    let absolute_frame = vat_frame(get_vat_data_safe(tag), globals.time);

    let vat = vat_sample(absolute_frame, vertex.uv_b);
    let new_position = vat_position(vat, vertex.position);

    let world_from_local = get_world_from_local(vertex.instance_index);

    // Local -> World (Position)
    out.world_position = mesh_position_local_to_world(world_from_local, vec4<f32>(new_position, 1.0));
    
    // World -> Clip
    out.position = position_world_to_clip(out.world_position.xyz);
    
    @if(VERTEX_UVS_A)
    out.uv = vertex.uv;

    @if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

    // Local -> World (Normal)
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_normal_local_to_world(vat_normal(vat), vertex.instance_index);

    return out;
}
