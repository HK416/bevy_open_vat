import bevy_pbr::{
    render::{
        mesh_functions::{get_tag, get_world_from_local, get_previous_world_from_local, mesh_position_local_to_world, mesh_normal_local_to_world, mesh_tangent_local_to_world, get_visibility_range_dither_level},
        view_transformations::position_world_to_clip,
    },
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
};
import bevy_render::globals::Globals;
import bevy_open_vat::openvat_common::{get_vat_data_safe, vat_normal, vat_position, vat_sample};

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
    let data = get_vat_data_safe(tag);
    let vat = vat_sample(data, globals.time, vertex.uv_b);
    let new_position = vat_position(vat, vertex.position);

    let world_from_local = get_world_from_local(vertex.instance_index);

    // Local -> World (Position)
    out.world_position = mesh_position_local_to_world(world_from_local, vec4<f32>(new_position, 1.0));

    // Where this vertex was last frame, for TAA / motion blur: the previous transform applied to
    // the pose sampled one frame earlier. Right after the shader time wraps this time is negative,
    // which is fine: the CPU folds the wrap into `offset`, so progress stays continuous across it.
    // Known limit: on the frame a playback change lands (play/seek/pause/set_speed) the instance
    // data already describes the new state, so that one frame's motion vector is off. Exact
    // vectors would need last frame's instance data in a second buffer.
    @if(MOTION_VECTOR_PREPASS) {
        let previous = vat_sample(data, globals.time - globals.delta_time, vertex.uv_b);
        let previous_position = vat_position(previous, vertex.position);
        out.previous_world_position = mesh_position_local_to_world(
            get_previous_world_from_local(vertex.instance_index),
            vec4<f32>(previous_position, 1.0),
        );
    }
    
    // World -> Clip
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
        out.unclipped_depth = out.position.z;
        out.position.z = min(out.position.z, 1.0); // Clamp depth to avoid clipping
    }
    
    @if(VERTEX_UVS_A)
    out.uv = vertex.uv;

    @if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

    // Local -> World (Normal, Tangent)
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS) {
        let new_normal = vat_normal(vat);
        out.world_normal = mesh_normal_local_to_world(new_normal, vertex.instance_index);
        @if(VERTEX_TANGENTS) {
            // Same NaN-safe re-orthogonalization as the main pass; see openvat_pbr.wesl.
            let t = vertex.tangent.xyz;
            let ortho = t - new_normal * dot(new_normal, t);
            let len_sq = dot(ortho, ortho);
            let safe_t = select(t, ortho * inverseSqrt(len_sq), len_sq > 1e-8);
            out.world_tangent = mesh_tangent_local_to_world(world_from_local, vec4<f32>(safe_t, vertex.tangent.w), vertex.instance_index);
        }
    }

    @if(VERTEX_COLORS)
    out.color = vertex.color;

    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;

    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);

    return out;
}
