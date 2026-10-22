// Native ordinary-open check for final reference-sized masters and fraction maps.
// Uses local expected probes; writes JSON only and never modifies the user's session.
function verifyReferenceMaps() {
    var root=File.extractDrive(#__FILE__)+File.extractDirectory(#__FILE__)+"/../..";
    var expected=JSON.parse(File.readTextFile(root+"/outputs/M4_reference_maps_PI_expected.json"));
    var path=root+"/outputs/M4_reference_maps_PI_validation.json";
    if(File.exists(path))throw new Error("Validation report already exists.");
    var report={status:"PASS",scope:"Ordinary import of six final RGB files; no intensity conversions or image writes; not owner visual approval",files:[]};
    for(var i=0;i<expected.files.length;++i){var item=expected.files[i];var opened;var entry={path:item.path,status:"FAIL"};
        try {
            opened=ImageWindow.open(root+"/"+item.path,"FulgurReferenceMap","verbosity 0",true)[0];var image=opened.mainView.image;
            if(image.width!==item.width || image.height!==item.height || image.numberOfChannels!==3 || opened.isModified)throw new Error("Unexpected dimensions, channels or modified image.");
            var samples=new Float32Array(image.width*image.height);var min=Infinity,max=-Infinity,mismatch=0,outside=0;
            for(var c=0;c<3;++c){image.getSamples(samples,new Rect(image.width,image.height),c);
                for(var n=0;n<samples.length;++n){var v=samples[n];if(!(v>=0 && v<=1))++outside;if(v<min)min=v;if(v>max)max=v;}
                for(var k=0;k<item.probes.length;++k){var p=item.probes[k];if(image.sample(p.x,p.y,c)!==p.rgb[c])++mismatch;}
            }
            if(outside!==0 || mismatch!==0 || min!==item.minimum || max!==item.maximum)throw new Error("Import changed fraction/flux values, range or orientation.");
            entry.status="PASS";entry.width=image.width;entry.height=image.height;entry.minimum=min;entry.maximum=max;entry.probeMismatches=mismatch;entry.outOfRange=outside;
        }catch(error){entry.error=String(error);report.status="FAIL";}
        finally{if(opened)opened.forceClose();}
        report.files.push(entry);
    }
    File.writeTextFile(path,JSON.stringify(report,null,2));
    if(report.status!=="PASS")throw new Error("Reference/map import validation failed.");
}
verifyReferenceMaps();
