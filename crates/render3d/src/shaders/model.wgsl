struct Camera { matrix: mat4x4<f32>, eye: vec4<f32> }
struct Material {
    base_color: vec4<f32>,
    emissive_metallic: vec4<f32>,
    factors: vec4<f32>,
    flags: vec4<u32>,
    uv_sets: vec4<u32>,
    uv_extra: vec4<u32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_map: texture_2d<f32>;
@group(1) @binding(2) var base_sampler: sampler;
@group(1) @binding(3) var mr_map: texture_2d<f32>;
@group(1) @binding(4) var mr_sampler: sampler;
@group(1) @binding(5) var normal_map: texture_2d<f32>;
@group(1) @binding(6) var normal_sampler: sampler;
@group(1) @binding(7) var occlusion_map: texture_2d<f32>;
@group(1) @binding(8) var occlusion_sampler: sampler;
@group(1) @binding(9) var emissive_map: texture_2d<f32>;
@group(1) @binding(10) var emissive_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
}
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>, @location(3) uv0: vec2<f32>, @location(4) uv1: vec2<f32>, @location(5) color: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.matrix * vec4<f32>(position, 1.0);
    out.position = position;
    out.normal = normal;
    out.tangent = tangent;
    out.uv0 = uv0;
    out.uv1 = uv1;
    out.color = color;
    return out;
}
fn uv(input: VertexOut, uv_set: u32) -> vec2<f32> { return select(input.uv0, input.uv1, uv_set == 1u); }
fn safe_normalize(v: vec3<f32>) -> vec3<f32> { return v * inverseSqrt(max(dot(v, v), 0.00000001)); }
fn tone_map(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}
@fragment fn fs_main(input: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Sample before any discard or varying branch so derivatives stay valid.
    let base = textureSample(base_map, base_sampler, uv(input, material.uv_sets.x)) * material.base_color * input.color;
    let mr = textureSample(mr_map, mr_sampler, uv(input, material.uv_sets.y));
    let normal_uv = uv(input, material.uv_sets.z);
    let sampled_normal = textureSample(normal_map, normal_sampler, normal_uv).xyz * 2.0 - 1.0;
    let occlusion = textureSample(occlusion_map, occlusion_sampler, uv(input, material.uv_sets.w)).r;
    let emissive = textureSample(emissive_map, emissive_sampler, uv(input, material.uv_extra.x)).rgb * material.emissive_metallic.xyz;
    let dx = dpdx(input.position);
    let dy = dpdy(input.position);
    let ux = dpdx(normal_uv);
    let uy = dpdy(normal_uv);
    var n = safe_normalize(input.normal);
    if material.flags.w == 1u && !front { n = -n; }
    if material.flags.z == 1u {
        var t = input.tangent.xyz;
        var sign = input.tangent.w;
        if dot(t, t) < 0.01 || material.uv_sets.z != 0u {
            let det = ux.x * uy.y - ux.y * uy.x;
            t = (dx * uy.y - dy * ux.y) * select(-1.0, 1.0, det >= 0.0);
            let raw_b = (dy * ux.x - dx * uy.x) * select(-1.0, 1.0, det >= 0.0);
            sign = select(-1.0, 1.0, dot(cross(n, t), raw_b) >= 0.0);
        }
        t = safe_normalize(t - n * dot(n, t));
        if dot(t, t) > 0.01 {
            let b = cross(n, t) * sign;
            n = safe_normalize(mat3x3<f32>(t, b, n) * vec3<f32>(sampled_normal.xy * material.factors.y, sampled_normal.z));
        }
    }
    if material.flags.x == 1u && base.a < material.factors.w { discard; }
    let alpha = select(1.0, base.a, material.flags.x == 2u);
    if material.flags.y == 1u { return vec4<f32>(base.rgb, alpha); }

    let metallic = clamp(material.emissive_metallic.w * mr.b, 0.0, 1.0);
    let roughness = clamp(material.factors.x * mr.g, 0.045, 1.0);
    let v = safe_normalize(camera.eye.xyz - input.position);
    let l = normalize(vec3<f32>(0.5, 0.8, 0.6));
    let h = safe_normalize(v + l);
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 0.001);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let a = roughness * roughness;
    let a2 = a * a;
    let denominator = nh * nh * (a2 - 1.0) + 1.0;
    let distribution = a2 / max(3.14159265 * denominator * denominator, 0.000001);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let geometry = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
    let f0 = mix(vec3<f32>(0.04), base.rgb, metallic);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - vh, 5.0);
    let specular = distribution * geometry * fresnel / max(4.0 * nv * nl, 0.0001);
    let diffuse = (1.0 - fresnel) * (1.0 - metallic) * base.rgb / 3.14159265;
    let ambient = (base.rgb * (1.0 - metallic) + f0 * 0.35) * 0.22 * mix(1.0, occlusion, material.factors.z);
    let color = ambient + (diffuse + specular) * nl * 3.0 + emissive;
    return vec4<f32>(tone_map(color), alpha);
}
