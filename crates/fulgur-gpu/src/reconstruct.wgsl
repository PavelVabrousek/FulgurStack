// Accepted CPU cosmetics and VNG; source ADU stays f32 and unbounded.
struct Job { offset: u32, width: u32, height: u32, pattern: u32 }
@group(0) @binding(0) var<storage, read> raw: array<f32>;
@group(0) @binding(1) var<storage, read_write> clean: array<f32>;
@group(0) @binding(2) var<storage, read_write> rgb: array<f32>;
@group(0) @binding(3) var<storage, read> jobs: array<Job>;
@group(0) @binding(4) var<storage, read> plans: array<u32>;
@group(0) @binding(5) var<storage, read_write> corrections: array<atomic<u32>>;

fn median8(input: array<f32,8>) -> f32 {
    var v = input;
    for (var i=1u; i<8u; i++) {
        let a=v[i]; var j=i;
        loop { if (j==0u) { break; } if (v[j-1u]<=a) { break; }
            v[j]=v[j-1u]; j--; }
        v[j]=a;
    }
    return (v[3]+v[4])*0.5;
}
fn neighborhood(job: Job, x: i32, y: i32) -> vec2<f32> {
    var v: array<f32,8>; var k=0u;
    for(var dy=-2;dy<=2;dy+=2) { for(var dx=-2;dx<=2;dx+=2) {
        if(dx!=0 || dy!=0) { v[k]=raw[job.offset+u32(y+dy)*job.width+u32(x+dx)]; k++; }
    }}
    let m=median8(v); var d: array<f32,8>;
    for(var i=0u;i<8u;i++) { d[i]=abs(v[i]-m); }
    return vec2(m,max(1.4826*median8(d),1.0));
}
fn refined_noise(job: Job, x:i32, y:i32) -> f32 {
    var v: array<f32,24>; var k=0u;
    for(var dy=-4;dy<=4;dy+=2) { for(var dx=-4;dx<=4;dx+=2) {
        if(dx!=0 || dy!=0) { v[k]=raw[job.offset+u32(y+dy)*job.width+u32(x+dx)]; k++; }
    }}
    for(var i=1u;i<24u;i++) { let a=v[i]; var j=i;
        loop { if(j==0u) {break;} if(v[j-1u]<=a) {break;} v[j]=v[j-1u];j--; } v[j]=a; }
    let m=(v[11]+v[12])*0.5;
    for(var i=0u;i<24u;i++) { v[i]=abs(v[i]-m); }
    for(var i=1u;i<24u;i++) { let a=v[i];var j=i;
        loop { if(j==0u) {break;} if(v[j-1u]<=a) {break;} v[j]=v[j-1u];j--; } v[j]=a; }
    return max(1.4826*0.5*(v[11]+v[12]),1.0);
}
fn stellar_support(job: Job,x:i32,y:i32,excess:f32) -> bool {
    let dirs=array<vec2<i32>,4>(vec2(-1,0),vec2(1,0),vec2(0,-1),vec2(0,1));
    var supported=0u;
    for(var i=0u;i<4u;i++) {
        let n=vec2(x,y)+dirs[i];
        if(n.x<2 || n.y<2 || n.x+2>=i32(job.width) || n.y+2>=i32(job.height)) {continue;}
        let stat=neighborhood(job,n.x,n.y);
        if(raw[job.offset+u32(n.y)*job.width+u32(n.x)]>stat.x+min(3.0*stat.y,0.125*excess)) { supported++; }
    }
    return supported>=2u;
}
@compute @workgroup_size(128)
fn cosmetic(@builtin(global_invocation_id) id:vec3<u32>) {
    let job=jobs[id.y]; if(id.x>=job.width*job.height) {return;}
    let x=id.x%job.width;let y=id.x/job.width;let index=job.offset+id.x;
    let original=raw[index]; clean[index]=original;
    if(x<4u || y<4u || x+4u>=job.width || y+4u>=job.height) {return;}
    let stat=neighborhood(job,i32(x),i32(y));
    if(original>stat.x+5.0*stat.y) {
        if(original>stat.x+5.0*max(stat.y,refined_noise(job,i32(x),i32(y))) && !stellar_support(job,i32(x),i32(y),original-stat.x)) {
            clean[index]=stat.x; atomicAdd(&corrections[id.y],1u);
        }
    }
}
fn reflect(p:i32,n:u32) -> u32 {
    if(p<0) {return u32(-p);} if(p>=i32(n)) {return u32(2*(i32(n)-1)-p);} return u32(p);
}
@compute @workgroup_size(128)
fn vng(@builtin(global_invocation_id) id:vec3<u32>) {
    // A wave processes one Bayer phase, so specialized stencil branches are uniform.
    let job=jobs[id.y];let columns=(job.width+1u)/2u;
    let x=(id.x%columns)*2u+id.z%2u;let y=(id.x/columns)*2u+id.z/2u;
    if(x>=job.width || y>=job.height) {return;}let phase=id.z;
    var window: array<f32,25>;
    for(var dy=-2;dy<=2;dy++) {for(var dx=-2;dx<=2;dx++) {
        window[u32((dy+2)*5+dx+2)]=clean[job.offset+reflect(i32(y)+dy,job.height)*job.width+reflect(i32(x)+dx,job.width)];
    }}
    var gradients:array<f32,8>;var minimum=3.402823e38;var maximum=0.0;
    for(var d=0u;d<8u;d++) {
        let base=((job.pattern*4u+phase)*8u+d)*68u;
        var g=0.0;
        for(var i=0u;i<plans[base];i++) {
            let k=base+4u+i*4u;
            g+=bitcast<f32>(plans[k+2u])*abs(window[plans[k]]-window[plans[k+1u]]);
        }
        gradients[d]=g;minimum=min(minimum,g);maximum=max(maximum,g);
    }
    let threshold=minimum+0.5*maximum;var sums=array<f32,3>(0.0,0.0,0.0);var count=0u;
    for(var d=0u;d<8u;d++) {
        if(gradients[d]<=threshold) {
            let base=((job.pattern*4u+phase)*8u+d)*68u;
            for(var i=0u;i<plans[base+1u];i++) {
                let k=base+36u+i*4u;
                let product=window[plans[k]]*bitcast<f32>(plans[k+2u]);
                sums[plans[k+1u]]+=product;
            }
            count++;
        }
    }
    let reds=array<u32,4>(1u,0u,2u,3u);let red=reds[job.pattern];
    var measured=1u;if(phase==red) {measured=0u;}else if(phase==(3u-red)) {measured=2u;}
    let out_index=(job.offset+y*job.width+x)*3u;
    for(var c=0u;c<3u;c++) { rgb[out_index+c]=window[12]+(sums[c]-sums[measured])/f32(count); }
    rgb[out_index+measured]=window[12];
}
