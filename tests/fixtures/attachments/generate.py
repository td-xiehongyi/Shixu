"""Deterministic fictional notices only. Run from any cwd with Pillow 12.3.0 / pypdf 6.10.0.
No downloads; no original user files are accepted. Manifests are reviewable ground truth.
"""
from pathlib import Path
import io,json,hashlib,zlib
from PIL import Image,ImageDraw,ImageFont,ImageFilter
ROOT=Path(__file__).resolve().parent
FONT='/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
font=ImageFont.truetype(FONT,36)
images=[]; pdfs=[]; canvases={}
def canvas(lines,size=(1000,360)):
    im=Image.new('RGB',size,'white'); d=ImageDraw.Draw(im); regions=[]
    for text,x,y in lines:
        d.text((x,y),text,font=font,fill='black')
        bb=d.textbbox((x,y),text,font=font)
        regions.append({'text':text,'bbox':[max(0,bb[0]-3),max(0,bb[1]-3),min(size[0],bb[2]+3),min(size[1],bb[3]+3)]})
    return im,regions
lines=[('Workshop 2026-11-20',50,50),('Room 42',50,140)]
def record(name,b,kind,regions=None,expected='readable',**kw):
    (ROOT/name).write_bytes(b)
    item={'file':name,'sha256':hashlib.sha256(b).hexdigest(),'expected':expected,'regions':regions or [],**kw}
    (images if kind=='image' else pdfs).append(item)
def image_case(name,lines=lines,fmt='PNG',mode=None,blur=0,crop=False,size=(1000,360),expected='readable'):
    im,regions=canvas(lines,size)
    if mode: im=im.convert(mode)
    if blur: im=im.filter(ImageFilter.GaussianBlur(blur))
    if crop:
        im=im.crop((140,0,1000,360)); regions=[{'text':'2026-11-20','bbox':[0,40,700,120]},{'text':'42','bbox':[0,135,200,200]}]
    out=io.BytesIO();im.save(out,format=fmt)
    record(name,out.getvalue(),'image',regions,expected);canvases[name]=im.convert('RGB')
image_case('notice.png');image_case('notice.jpg',fmt='JPEG')
cn=[('虚构通知 2026年11月20日',50,50),('教室 42',50,140)]
image_case('chinese.png',cn);image_case('chinese.jpg',cn,fmt='JPEG')
image_case('multiline.png',[('Workshop',50,40),('2026-11-20',50,120),('Room 42',50,200)])
image_case('table.png',[('Workshop',50,50),('2026-11-20',550,50),('Lecture',50,170),('2026-11-21',550,170)])
image_case('columns.png',[('Workshop',50,50),('Lecture',600,50),('2026-11-20',50,170),('2026-11-21',600,170)])
image_case('small.png',size=(600,250));image_case('large.png',size=(1800,1000))
image_case('grayscale.png',mode='L');image_case('alpha.png',mode='RGBA')
image_case('blurred.png',blur=2.2,expected='degraded')
image_case('cropped.png',crop=True,expected='degraded')
image_case('blank.png',[],expected='recognition_failed')
image_case('wide.png',[],size=(10001,1),expected='limit_exceeded')
image_case('pixels.png',[],size=(6000,4000),expected='limit_exceeded')
record('malformed.png',b'\x89PNG\r\n\x1a\ninvalid','image',expected='recognition_failed')
record('truncated.jpg',(ROOT/'notice.jpg').read_bytes()[:70],'image',expected='recognition_failed')
b=bytearray((ROOT/'notice.png').read_bytes());b[29]^=255
record('bad_crc.png',b,'image',expected='recognition_failed')
record('unsupported.gif',b'GIF89a'+b'\0'*40,'image',expected='unsupported')

def pdf_bytes(pages,extra=b'',special=[]):
    objects=[b'',b'']; kids=[]; allregions=[]
    for spec in pages:
        num=len(objects)+1; kids.append(f'{num} 0 R'); objects.extend([b'',b''])
        w,h=spec.get('size',(1000,360)); res='<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>'
        if 'image' in spec:
            im=canvases[spec['image']];w,h=im.size; raw=zlib.compress(im.tobytes());imnum=len(objects)+1
            objects.append(f'<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {len(raw)} >>\nstream\n'.encode()+raw+b'\nendstream')
            res=f'<< /XObject << /Im0 {imnum} 0 R >> >>'; stream=f'q {w} 0 0 {h} 0 0 cm /Im0 Do Q'.encode()
            regs=next(x['regions'] for x in images if x['file']==spec['image'])
        else:
            stream=b'';regs=[]
            for text,x,y in spec.get('lines',lines):
                stream+=f'BT /F1 36 Tf 1 0 0 1 {x} {h-y-36} Tm ({text}) Tj ET\n'.encode()
                regs.append({'text':text,'bbox':[x-3,y-3,min(w,x+len(text)*28),y+48]})
        objects[num-1]=f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources {res} /Contents {num+1} 0 R >>'.encode()
        objects[num]=f'<< /Length {len(stream)} >>\nstream\n'.encode()+stream+b'\nendstream'
        allregions.extend([{**r,'page':len(kids)} for r in regs])
    objects[0]=b'<< /Type /Catalog /Pages 2 0 R '+extra+b' >>'
    objects[1]=f'<< /Type /Pages /Count {len(kids)} /Kids [{" ".join(kids)}] >>'.encode()
    objects+=special
    out=b'%PDF-1.7\n';offsets=[0]
    for i,obj in enumerate(objects,1):
        offsets.append(len(out));out+=f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n'
    start=len(out);out+=f'xref\n0 {len(objects)+1}\n0000000000 65535 f \n'.encode()
    for offset in offsets[1:]:out+=f'{offset:010} 00000 n \n'.encode()
    out+=f'trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n'.encode()
    return out,allregions

def pdf_case(name,pages,expected='readable',extra=b'',special=[]):
    b,r=pdf_bytes(pages,extra,special);record(name,b,'pdf',r,expected,pages=len(pages));return b
pdf_case('native.pdf',[{}]);pdf_case('scan.pdf',[{'image':'notice.png'}]);pdf_case('mixed.pdf',[{}, {'image':'notice.png'}])
pdf_case('chinese_scan.pdf',[{'image':'chinese.png'}]);pdf_case('multiline.pdf',[{'lines':[('Workshop',50,40),('2026-11-20',50,120),('Room 42',50,200)]}])
pdf_case('table.pdf',[{'image':'table.png'}]);pdf_case('columns.pdf',[{'image':'columns.png'}]);pdf_case('jpeg_scan.pdf',[{'image':'notice.jpg'}])
pdf_case('blur_scan.pdf',[{'image':'blurred.png'}],expected='degraded');pdf_case('crop_scan.pdf',[{'image':'cropped.png'}],expected='degraded')
pdf_case('blank.pdf',[{'lines':[]}],expected='recognition_failed')
pdf_case('twenty.pdf',[{}]*20);pdf_case('twenty_one.pdf',[{}]*21,expected='limit_exceeded')
pdf_case('huge_page.pdf',[{'size':(10001,360)}],expected='limit_exceeded')
pdf_case('actions.pdf',[{}],extra=br'/OpenAction << /S /JavaScript /JS (app.launchURL\("http://127.0.0.1:9/shixu-synthetic-probe"\);) >> /AA << /WC << /S /Launch /F (/tmp/shixu-n4-must-not-open) >> >>')
pdf_case('embedded.pdf',[{}],extra=b'/Names << /EmbeddedFiles << /Names [(probe.txt) << /Type /Filespec /F (/tmp/shixu-n4-must-not-open) /EF << /F 5 0 R >> >>] >> >>',special=[b'<< /Type /EmbeddedFile /Length 9 >>\nstream\nSYNTHETIC\nendstream'])
pdf_case('xfa.pdf',[{}],extra=b'/AcroForm << /XFA 5 0 R >>',special=[b'<< /Length 31 >>\nstream\n<xfa>synthetic-disabled</xfa>\nendstream'])
record('malformed.pdf',b'%PDF-1.7\ngarbage','pdf',expected='recognition_failed')
record('truncated.pdf',b'%PDF-1.7\n1 0 obj << /Type /Catalog','pdf',expected='recognition_failed')
from pypdf import PdfReader,PdfWriter
from pypdf.generic import ArrayObject,ByteStringObject
wr=PdfWriter();wr.append(PdfReader(io.BytesIO((ROOT/'native.pdf').read_bytes())))
wr._ID=ArrayObject([ByteStringObject(b'SYNTHETIC-N4-001'),ByteStringObject(b'SYNTHETIC-N4-001')]);wr.encrypt('fictional-fixture-only',algorithm='RC4-128');out=io.BytesIO();wr.write(out)
record('encrypted.pdf',out.getvalue(),'pdf',expected='auth_required')
for kind,data in [('image',images),('pdf',pdfs)]:
    assert len(data)>=20
    (ROOT/(kind+'_manifest.json')).write_text(json.dumps({'synthetic_only':True,'schema':1,'fixtures':data},ensure_ascii=False,indent=2)+'\n')
print('generated',len(images),'images',len(pdfs),'PDFs')
