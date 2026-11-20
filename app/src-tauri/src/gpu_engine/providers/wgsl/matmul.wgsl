// matmul: C(i,j) = sum_t A(i,t) * B(t,j)
//   x = A (m*k), y = B (k*n), out = C (m*n)
//   params.p0 = m, params.p1 = k, params.p2 = n
// Naive scalar kernel, ascending-t accumulation. Grid-stride over the linear
// output index so a bounded workgroup count covers up to m*n outputs.
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
    let m = params.p0;
    let k = params.p1;
    let n = params.p2;
    let total = m * n;
    let stride = nwg.x * 64u;
    var idx = gid.x;
    loop {
        if (idx >= total) {
            break;
        }
        let i = idx / n;
        let j = idx - i * n;
        var sum: f32 = 0.0;
        for (var t: u32 = 0u; t < k; t = t + 1u) {
            let prod = x[i * k + t] * y[t * n + j];
            sum = sum + prod;
        }
        out[idx] = sum;
        idx = idx + stride;
    }
}
