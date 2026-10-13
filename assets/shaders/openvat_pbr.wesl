import bevy_pbr::render::{
    mesh_functions::{get_tag, get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world},
    view_transformations::position_world_to_clip,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_view_bindings::globals,
};
import bevy_open_vat::openvat_common::{get_vat_data_safe, vat_normal, vat_position, vat_sample};

// --- Vertex Shader ---

@vertex
fn main(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Handles meshes with compressed vertex attributes (`Mesh::compress_*`)
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);

    let tag = get_tag(vertex.instance_index);
    let vat = vat_sample(get_vat_data_safe(tag), globals.time, vertex.uv_b);
    let new_position = vat_position(vat, vertex.position);
    let new_normal = vat_normal(vat);

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
