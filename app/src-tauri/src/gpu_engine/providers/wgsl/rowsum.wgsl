// row_sum: out[i] = sum_c A(i,c)
//   x = A (rows*cols), y declared for a shared layout (unused)
//   params.p0 = rows, params.p1 = cols
// One invocation per row, ascending-column accumulation (plain adds, matching
// the CPU reference exactly). Grid-stride over rows.
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
    let rows = params.p0;
    let cols = params.p1;
    let stride = nwg.x * 64u;
    var i = gid.x;
    loop {
        if (i >= rows) {
            break;
        }
        var sum: f32 = 0.0;
        for (var c: u32 = 0u; c < cols; c = c + 1u) {
            sum = sum + x[i * cols + c];
        }
        out[i] = sum;
        i = i + stride;
    }
}
