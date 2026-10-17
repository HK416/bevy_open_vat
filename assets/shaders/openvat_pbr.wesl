import bevy_pbr::render::{
    mesh_functions::{get_tag, get_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world, mesh_tangent_local_to_world, get_visibility_range_dither_level},
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

    @if(VERTEX_TANGENTS) {
        // Tangents are not baked into the VAT, so the rest-pose tangent is re-orthogonalized
        // against the animated normal. When the two become parallel the projection vanishes;
        // keep the rest tangent then instead of normalizing a zero vector into NaN.
        let t = vertex.tangent.xyz;
        let ortho = t - new_normal * dot(new_normal, t);
        let len_sq = dot(ortho, ortho);
        let safe_t = select(t, ortho * inverseSqrt(len_sq), len_sq > 1e-8);
        out.world_tangent = mesh_tangent_local_to_world(world_from_local, vec4<f32>(safe_t, vertex.tangent.w), vertex.instance_index);
    }

    @if(VERTEX_COLORS)
    out.color = vertex.color;

    // The fragment shader reads this mesh's flags and material slot through `instance_index`.
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;

    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);

    return out;
}
