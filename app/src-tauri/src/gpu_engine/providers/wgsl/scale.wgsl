// scale: out[i] = a * x[i]   (y binding declared for a shared layout, unused)
struct Params {
    p0: u32,
    p1: u32,
    p2: u32,
    a:  f32,
};

@group(0) @binding(0) var<storage, read>       x:   array<f32>;
@group(0) @binding(1) var<storage, read>       y:   array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<uniform>            params: Params;

@compute @workgroup_size(64)
