# Development-only DS-B verification; requires NumPy, never modifies input images.
import hashlib,json,datetime,math
from pathlib import Path
import numpy as np

def load(p):return json.loads(Path(p).read_text(encoding='utf-8-sig'))
def digest(p):
 h=hashlib.sha256()
 with Path(p).open('rb') as f:
  while b:=f.read(1048576):h.update(b)
 return h.hexdigest()
def header(f):
 cards=[]
 while True:
  b=f.read(2880);assert len(b)==2880;cards.extend([b[i:i+80] for i in range(0,2880,80)])
  if any(c.startswith(b'END ') for c in cards):break
 d={}
 for c in cards:
  if c.startswith(b'END '):break
  if c[8:10]==b'= ':d[c[:8].decode().strip()]=c
 return d

def value(h,k):return h[k][10:].decode().split('/')[0].strip().strip("'").strip()
def image(p):
 with Path(p).open('rb') as f:
  h=header(f);assert value(h,'BITPIX')=='-32' and value(h,'NAXIS3')=='3'
  w=int(value(h,'NAXIS1'));y=int(value(h,'NAXIS2'));a=np.frombuffer(f.read(w*y*12),dtype='>f4').reshape(3,y,w)
 assert np.isfinite(a).all() and a.min()>=0 and a.max()<=1 and Path(p).stat().st_size%2880==0
 return h,a

def date(s):
 d=datetime.datetime.fromisoformat(s.replace('Z','+00:00'))
 return d.replace(tzinfo=datetime.timezone.utc) if d.tzinfo is None else d.astimezone(datetime.timezone.utc)
results={};expected={'files':[]}
for tag in ['median','PI']:
 stem='M4_DS-B_reference_'+tag;r=load('outputs/'+stem+'.json');reg=r['registration'];n=reg['frame_count'];accepted=reg['accepted_count'];assert n==32 and accepted==27
 rows=[f for f in reg['inspection']['frames'] if f['accepted']];rows.sort(key=lambda f:(date(f['frame']['date_obs']),f['file_name']))
 selected=rows[(len(rows)-1)//2]['file_name'] if tag=='median' else 'Light_M 33_10.0s_IRCUT_20261006-232807.fit'
 assert reg['reference_anchor']==selected;assert r['width']==2160 and r['height']==3840 and r['origin_x_px']==r['origin_y_px']==0;assert r['canvas_mode']=='reference';assert r['map_denominator_frames']==32
 with (Path('SeestarS50 Subs')/selected).open('rb') as f:anchor=header(f)
 mh,master=image('outputs/'+r['output_file']);ch,cov=image('outputs/'+r['coverage_file']);hh,high=image('outputs/'+r['high_rejection_file'])
 assert master.shape==cov.shape==high.shape==(3,3840,2160)
 assert int(value(mh,'CROPX'))==int(value(mh,'CROPY'))==0 and int(value(mh,'NCOMBINE'))==27
 assert float(value(mh,'EXPTIME'))==10 and float(value(mh,'TEXPTIME'))==270
 for k in ['SITELAT','SITELONG']:assert mh[k]==anchor[k]
 for h,kind in [(ch,'COVERAGE'),(hh,'HIGHREJ')]:
  assert value(h,'MAPTYPE')==kind and value(h,'BUNIT')=='fraction' and int(value(h,'NINPUT'))==32 and float(value(h,'PCTFACT'))==100
  for key in ['FLXDIV','BZERO','BSCALE','SITELAT','SITELONG']:assert key not in h
 assert (cov<=accepted/n).all() and (high<=accepted/n).all() and (cov+high<=accepted/n).all()
 assert np.array_equal(cov*n,np.rint(cov*n)) and np.array_equal(high*n,np.rint(high*n))
 used=int(cov.sum(dtype=np.float64)*n);upper=int(high.sum(dtype=np.float64)*n)
 assert upper==r['integration']['rejected_high_channel_samples']
 assert used+upper+r['integration']['rejected_low_channel_samples']==r['integration']['valid_channel_samples']
 assert (master[cov==0]==0).all();assert np.any(cov[0]!=cov[1]) and np.any(high[0]!=high[1])
 assert r['integration']['planned_buffer_bytes']<=r['config']['memory_budget_mib']*1048576;assert r['peak_rss_bytes']<=r['config']['memory_budget_mib']*1048576
 text=Path('outputs/'+stem+'.json').read_text();assert 'SITELAT' not in text and 'SITELONG' not in text
 stats=[]
 for name,a in [('master',master),('coverage',cov),('high_rejection',high)]:
  filename=r['output_file'] if name=='master' else r['coverage_file'] if name=='coverage' else r['high_rejection_file']
  probes=[{'x':x,'y':y,'rgb':[float(v) for v in a[:,y,x]]} for y in [0,1,117,1409,1919,3838,3839] for x in [0,1,500,1079,1694,2158,2159]]
  expected['files'].append({'path':'outputs/'+filename,'width':2160,'height':3840,'minimum':float(a.min()),'maximum':float(a.max()),'probes':probes})
  stats.append({'kind':name,'path':'outputs/'+filename,'sha256':digest('outputs/'+filename),'minimum':float(a.min()),'maximum':float(a.max())})
 results[tag]={'reference':selected,'reference_DATE_OBS':reg['reference_date_obs'],'dimensions':[2160,3840],'accepted':accepted,'original':n,'processing_seconds':r['wall_seconds'],'peak_MiB':r['peak_rss_bytes']/1048576,'sample_accounting':'PASS; maps exactly reconstruct global used/high/low counts','used_channel_samples':used,'high_rejected_channel_samples':upper,'coverage_max_percent':float(cov.max())*100,'coverage_mean_percent':float(cov.mean(dtype=np.float64))*100,'high_rejection_max_percent':float(high.max())*100,'products':stats}
manifest=load('docs/ds-b-manifest.json')
for entry in manifest['sources']+manifest['references']:assert digest(entry['path'])==entry['sha256']
assert not list(Path('outputs').glob('*.tmp'))
results['source_integrity']='PASS; all 34 original/reference hashes unchanged';results['temporary_files']='PASS; none remain'
with Path('outputs/M4_reference_maps_validation.json').open('x',encoding='utf-8') as f:json.dump(results,f,indent=2)
with Path('outputs/M4_reference_maps_PI_expected.json').open('x',encoding='utf-8') as f:json.dump(expected,f)
print(json.dumps({k:{x:y for x,y in v.items() if x!='products'} if isinstance(v,dict) else v for k,v in results.items()},indent=2))
