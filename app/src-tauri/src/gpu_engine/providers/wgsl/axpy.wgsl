// Phase 3 / Subagent 2 — axpy: out[i] = a * x[i] + y[i]
// Layout (identical across all four shaders, matches WorkloadInputs + a params
// uniform): binding 0 = x, 1 = y, 2 = out, 3 = Params.
// params.p0 = number of elements in this dispatch (n / chunk_n).
// Grid-stride loop so a bounded number of workgroups covers any n.
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
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(num_workgroups)       nwg: vec3<u32>) {
    let n = params.p0;
    let stride = nwg.x * 64u;
    var i = gid.x;
    loop {
        if (i >= n) {
            break;
        }
        out[i] = params.a * x[i] + y[i];
        i = i + stride;
    }
}
