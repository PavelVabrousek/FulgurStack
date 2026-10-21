// Native ordinary-open M4 contract. Writes JSON only, never an image or user preferences.
function verifyMasterImport() {
    var root=File.extractDrive(#__FILE__)+File.extractDirectory(#__FILE__)+"/../..";
    var expected=JSON.parse(File.readTextFile(root+"/outputs/M4_PI_expected.json"));
    var reportPath=root+"/outputs/M4_PI_import_validation.json";
    if(File.exists(reportPath)) throw new Error("Validation report already exists.");
    var report={status:"FAIL",scope:"Ordinary File > Open import and direct native samples; no STF or intensity conversion; not owner visual approval"};
    var opened;
    function require(ok,message){if(!ok)throw new Error(message);}
    try {
        opened=ImageWindow.open(root+"/outputs/M4_DS-B_master.fits","FulgurMasterReview","verbosity 0",true)[0];
        var image=opened.mainView.image;
        require(image.width===expected.width && image.height===expected.height && image.numberOfChannels===3,"Unexpected RGB dimensions.");
        require(!opened.isModified,"Imported master was modified.");
        report.loadedMinimum=Infinity;report.loadedMaximum=-Infinity;report.outOfRange=0;report.probeMismatches=0;
        var samples=new Float32Array(image.width*image.height);
        for(var c=0;c<3;++c){
            image.getSamples(samples,new Rect(image.width,image.height),c);
            for(var n=0;n<samples.length;++n){var v=samples[n];if(!(v>=0 && v<=1))++report.outOfRange;if(v<report.loadedMinimum)report.loadedMinimum=v;if(v>report.loadedMaximum)report.loadedMaximum=v;}
            for(var i=0;i<expected.probes.length;++i){var p=expected.probes[i];if(image.sample(p.x,p.y,c)!==p.rgb[c])++report.probeMismatches;}
        }
        report.width=image.width;report.height=image.height;report.comparedSamples=3*samples.length;
        report.gpsPresent=false;var lat=false,lon=false;
        for(var k=0;k<opened.keywords.length;++k){var keyword=opened.keywords[k];if(keyword.name==="SITELAT")lat=true;if(keyword.name==="SITELONG")lon=true;}
        report.gpsPresent=lat&&lon;
        require(report.outOfRange===0 && report.probeMismatches===0,"Ordinary import changed range or pixel orientation.");
        require(report.loadedMinimum===expected.minimum && report.loadedMaximum===expected.maximum,"Ordinary import changed the serialized range.");
        require(report.gpsPresent===expected.gpsPresent,"Anchor GPS presence changed.");
        report.status="PASS";
    }catch(error){report.error=String(error);}
    finally{if(opened)opened.forceClose();}
    File.writeTextFile(reportPath,JSON.stringify(report,null,2));
    require(report.status==="PASS",report.error||"Master import failed.");
}
verifyMasterImport();
