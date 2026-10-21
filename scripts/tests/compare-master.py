# Development-only read-only quality comparison (requires NumPy; not a product dependency).
import argparse,json,math,statistics
from pathlib import Path
parser=argparse.ArgumentParser(description="Compare a DS-B master with the immutable PI master without modifying either image.")
parser.add_argument("--master",default="outputs/M4_DS-B_master.fits")
parser.add_argument("--registration-report",default="outputs/M4_DS-B_report.json")
parser.add_argument("--reference",default="PixInsight Files/integration.fit")
parser.add_argument("--report",required=True)
args=parser.parse_args()
if Path(args.report).exists():raise SystemExit("Destination already exists; choose a new report filename.")
import numpy as np

def read(path):
 with Path(path).open('rb') as f:
  cards=[]
  while True:
   block=f.read(2880);assert len(block)==2880
   cards.extend([block[i:i+80].decode('ascii') for i in range(0,2880,80)])
   if any(c.startswith('END ') for c in cards):break
  header={}
  for c in cards:
   if c.startswith('END '):break
   if c[8:10]=='= ':header[c[:8].strip()]=c[10:].split('/')[0].strip().strip("'").strip()
  assert header['BITPIX']=='-32'
  shape=(int(header['NAXIS3']),int(header['NAXIS2']),int(header['NAXIS1']))
  data=np.frombuffer(f.read(np.prod(shape)*4),dtype='>f4').reshape(shape).astype(float)
  if header.get('ROWORDER')=='BOTTOM-UP':data=data[:,::-1,:]
  return header,data

def measure(a,p):
 x,y=p;ix=int(round(x));iy=int(round(y))
 if ix<15 or iy<15 or ix+16>=a.shape[2] or iy+16>=a.shape[1]:return None
 yy,xx=np.mgrid[iy-15:iy+16,ix-15:ix+16];rr=(xx-x)**2+(yy-y)**2;patch=a[:,iy-15:iy+16,ix-15:ix+16]
 bg=np.median(patch[:,(rr>=100)&(rr<196)],axis=1)
 sub=patch-bg[:,None,None];flux=sub[:,rr<64].sum(axis=1)
 green=np.maximum(sub[1],0)*(rr<36);total=green.sum()
 cx=(green*xx).sum()/total;cy=(green*yy).sum()/total
 trace=(green*((xx-cx)**2+(yy-cy)**2)).sum()/total;fwhm=2.35482*math.sqrt(trace/2)
 return {'flux':flux.tolist(),'fwhm':fwhm,'centroid':[cx,cy],'peak':float(patch.max())}

r=json.loads(Path(args.registration_report).read_text());plan=r['registration'];ref=next(f for f in plan['inspection']['frames'] if f['file_name']==plan['reference_anchor'])
template='Light_M 33_10.0s_IRCUT_20261006-232807.fit';t=next(f for f in plan['frames'] if f['file_name']==template)['transform'];c=math.cos(t['theta_rad']);s=math.sin(t['theta_rad'])
ha,a=read(args.master);hb,b=read(args.reference);samples=[]
for star in sorted(ref['stars'],key=lambda s:-s['flux_adu']):
 if star['snr']<100:continue
 x=star['x_px'];y=star['y_px'];p=[x-r['origin_x_px'],y-r['origin_y_px']];q=[c*(x-t['tx_px'])+s*(y-t['ty_px']),-s*(x-t['tx_px'])+c*(y-t['ty_px'])]
 if any(math.hypot(x-o['x_px'],y-o['y_px'])<28 for o in ref['stars'] if o is not star):continue
 ma=measure(a,p);mb=measure(b,q)
 if ma is None or mb is None or max(ma['peak'],mb['peak'])>=0.9 or min(ma['flux']+mb['flux'])<=0:continue
 da=math.hypot(ma['centroid'][0]-p[0],ma['centroid'][1]-p[1]);db=math.hypot(mb['centroid'][0]-q[0],mb['centroid'][1]-q[1])
 samples.append({'anchor':[x,y],'master':p,'pi':q,'ours':ma,'reference':mb,'centroid_offset_ours':da,'centroid_offset_pi':db})
 if len(samples)>=40:break
assert len(samples)>=8,len(samples)
# Check the geometric relationship before comparing profiles. No image or reference is modified.
scalar=statistics.median(v['reference']['flux'][1]/v['ours']['flux'][1] for v in samples)
flux_errors=[abs(v['ours']['flux'][1]*scalar/v['reference']['flux'][1]-1) for v in samples]
fwhm_ratios=[v['ours']['fwhm']/v['reference']['fwhm'] for v in samples]
colors=[]
for ch in [0,2]:colors.append(statistics.median((v['ours']['flux'][ch]/v['ours']['flux'][1])/(v['reference']['flux'][ch]/v['reference']['flux'][1])-1 for v in samples))
# Fixed spatial grid, source-free patches based on both images' robust clipping fraction.
sky=[]
for y in range(100,a.shape[1]-100,180):
 for x in range(100,a.shape[2]-100,180):
  ax=x+r['origin_x_px'];ay=y+r['origin_y_px']
  if math.hypot((ax-1080)/1.0,(ay-1920)/1.5)<500:continue
  if any(math.hypot(ax-st['x_px'],ay-st['y_px'])<55 for st in ref['stars']):continue
  qx=c*(ax-t['tx_px'])+s*(ay-t['ty_px']);qy=-s*(ax-t['tx_px'])+c*(ay-t['ty_px']);qx=int(round(qx));qy=int(round(qy))
  if qx<20 or qy<20 or qx+20>=b.shape[2] or qy+20>=b.shape[1]:continue
  patches=[a[1,y-16:y+17,x-16:x+17],b[1,qy-16:qy+17,qx-16:qx+17]];sig=[];clean=True
  for patch in patches:
   v=patch.ravel();med=np.median(v);noise=1.4826*np.median(abs(v-med));fraction=np.mean(abs(v-med)>4*noise)
   if noise<=0 or fraction>0.02:clean=False;break
   # remove a fitted local sky plane before robust noise measurement, not from the scientific output.
   yy,xx=np.indices(patch.shape);design=np.column_stack([np.ones(v.size),xx.ravel(),yy.ravel()]);coeff=np.linalg.lstsq(design,v,rcond=None)[0];res=v-design@coeff;sig.append(float(1.4826*np.median(abs(res-np.median(res)))))
  if clean:sky.append({'anchor':[ax,ay],'pi':[qx,qy],'ours_sigma':sig[0],'pi_sigma':sig[1],'ratio':sig[0]*scalar/sig[1]})
metrics={'star_count':len(samples),'sky_patch_count':len(sky),'comparison_scalar':scalar,'median_centroid_offset_ours_px':statistics.median(v['centroid_offset_ours'] for v in samples),'median_centroid_offset_pi_px':statistics.median(v['centroid_offset_pi'] for v in samples),'median_absolute_flux_residual':statistics.median(flux_errors),'median_fwhm_ratio':statistics.median(fwhm_ratios),'median_RG_fractional_difference':colors[0],'median_BG_fractional_difference':colors[1],'median_sky_noise_ratio':statistics.median(v['ratio'] for v in sky) if sky else None}
metrics['thresholds']={'flux_residual':metrics['median_absolute_flux_residual']<=0.01,'fwhm':metrics['median_fwhm_ratio']<=1.05,'color':max(abs(x) for x in colors)<=0.05,'sky_noise':bool(sky) and metrics['median_sky_noise_ratio']<=1.10}
report={'status':'PASS' if all(metrics['thresholds'].values()) else 'REVIEW','scope':'DS-B final candidate read-only comparison; 27 accepted sources versus PI 32; distinct rejection/weights; one scalar for comparison only','metrics':metrics,'stars':samples,'sky_patches':sky}
Path(args.report).write_text(json.dumps(report,indent=2),encoding='utf-8',newline='\n');print(json.dumps(metrics,indent=2))
