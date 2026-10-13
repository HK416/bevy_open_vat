import bevy_pbr::render::{
    mesh_functions::{get_tag, get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    view_transformations::position_world_to_clip,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_view_bindings::globals,
};
import bevy_open_vat::openvat_common::{apply_vat, get_vat_data_safe, vat_frame};

// --- Vertex Shader ---

@vertex
fn main(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Handles meshes with compressed vertex attributes (`Mesh::compress_*`)
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);

    let tag = get_tag(vertex.instance_index);
    let absolute_frame = vat_frame(get_vat_data_safe(tag), globals.time);

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
    
    @if(VERTEX_UVS_A)
    out.uv = vertex.uv;

    @if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

    return out;
}
