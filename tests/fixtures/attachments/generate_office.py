"""Deterministic synthetic OOXML corpus; never downloads or opens Office."""
from pathlib import Path
import zipfile,io,json,hashlib,html
ROOT=Path(__file__).parent
W='http://schemas.openxmlformats.org/wordprocessingml/2006/main'
S='http://schemas.openxmlformats.org/spreadsheetml/2006/main'
R='http://schemas.openxmlformats.org/officeDocument/2006/relationships'
P='http://schemas.openxmlformats.org/package/2006/relationships'
C='http://schemas.openxmlformats.org/package/2006/content-types'
D='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'
X='application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml'
def rels(items):
 return '<Relationships xmlns="'+P+'">'+''.join('<Relationship Id="'+i+'" Type="'+R+'/'+t+'" Target="'+v+'"'+(' TargetMode="External"' if ex else '')+'/>' for i,t,v,ex in items)+'</Relationships>'
def archive(parts):
 out=io.BytesIO()
 with zipfile.ZipFile(out,'w',zipfile.ZIP_STORED) as z:
  for n,b in parts.items():
   info=zipfile.ZipInfo(n,(2026,10,9,0,0,0));info.external_attr=0o100644<<16;z.writestr(info,b)
 return out.getvalue()
def types(main,fmt,extras=()):
 return '<Types xmlns="'+C+'"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/'+main+'" ContentType="'+fmt+'"/>'+''.join('<Override PartName="/'+p+'" ContentType="'+t+'"/>' for p,t in extras)+'</Types>'
def p(t):return '<w:p><w:r><w:t>'+html.escape(t)+'</w:t></w:r></w:p>'
def doc(body,extra=None,ns=W,main_type=D):
 d={'[Content_Types].xml':types('word/document.xml',main_type),'_rels/.rels':rels([('root','officeDocument','word/document.xml',False)]),'word/document.xml':'<w:document xmlns:w="'+ns+'" xmlns:r="'+R+'" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body>'+body+'</w:body></w:document>'}
 d.update(extra or {});return d
def cell(t,ref='A1',kind='inlineStr',style=None):
 return '<c r="'+ref+'" t="'+kind+'"'+(' s="'+str(style)+'"' if style is not None else '')+'>'+('<is><t>'+html.escape(t)+'</t></is>' if kind=='inlineStr' else '<v>'+html.escape(t)+'</v>')+'</c>'
def book(body,extra=None,date1904=False,sheet_attrs='',sheet_extra='',style=None):
 extras=[('xl/worksheets/sheet1.xml','application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml')]
 rr=[('sheet','worksheet','worksheets/sheet1.xml',False)]
 if style is not None:
  extras.append(('xl/styles.xml','application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml'));rr.append(('styles','styles','styles.xml',False))
 d={'[Content_Types].xml':types('xl/workbook.xml',X,extras),'_rels/.rels':rels([('root','officeDocument','xl/workbook.xml',False)]),'xl/workbook.xml':'<workbook xmlns="'+S+'" xmlns:r="'+R+'"><workbookPr date1904="'+('1' if date1904 else '0')+'"/><sheets><sheet name="通知" sheetId="1" r:id="sheet" '+sheet_attrs+'/></sheets></workbook>','xl/_rels/workbook.xml.rels':rels(rr),'xl/worksheets/sheet1.xml':'<worksheet xmlns="'+S+'">'+sheet_extra+'<sheetData>'+body+'</sheetData></worksheet>'}
 if style is not None:d['xl/styles.xml']='<styleSheet xmlns="'+S+'">'+style+'</styleSheet>'
 d.update(extra or {});return d
def row(c,attrs=''):return '<row r="1" '+attrs+'>'+c+'</row>'
EVENT='2026年10月12日9:00高数考试'
CASES={f:[] for f in ['docx','xlsx']}
def add(fmt,name,parts,text,status='Success',gold=None,event_gold=None):
 b=archive(parts);file='office/'+name+'.'+fmt;(ROOT/file).write_bytes(b)
 source=text if gold is None else gold
 # Events are independently assigned from source intent, including supported omissions.
 events=event_gold if event_gold is not None else ([{'title':'高数考试','precision':'Exact','local_date':'2026-10-12'}] if EVENT in source else [])
 CASES[fmt].append(dict(id=name,file=file,sha256=hashlib.sha256(b).hexdigest(),expected_extracted=text,source_gold=source,status=status,events=events))
add('docx','paragraph',doc(p(EVENT)),EVENT)
add('docx','split_runs',doc('<w:p><w:r><w:t>2026年10月12日</w:t></w:r><w:r><w:t>9:00高数考试</w:t></w:r></w:p>'),EVENT)
add('docx','table',doc('<w:tbl><w:tr><w:tc>'+p(EVENT)+'</w:tc><w:tc>'+p('教室A301')+'</w:tc></w:tr></w:tbl>'),EVENT+'\n教室A301')
add('docx','unicode',doc(p('欢迎学生，资料📚')), '欢迎学生，资料📚')
add('docx','escaped',doc(p('A&B <通知>')), 'A&B <通知>')
add('docx','yearless',doc(p('10月12日高数考试')),'10月12日高数考试',event_gold=[{'title':'高数考试','precision':'UnknownDate','local_date':None}])
add('docx','dateonly',doc(p('2026年10月12日高数考试')),'2026年10月12日高数考试',event_gold=[{'title':'高数考试','precision':'DateOnly','local_date':'2026-10-12'}])
add('docx','negated',doc(p('明天不考试')),'明天不考试')
add('docx','question',doc(p('明天考试吗？')),'明天考试吗？')
add('docx','tracked',doc(p('正常')+'<w:ins>'+p(EVENT)+'</w:ins>'),'正常','PartialParse',gold='正常\n'+EVENT)
add('docx','deleted',doc(p('正常')+'<w:del>'+p('旧内容')+'</w:del>'),'正常','PartialParse',gold='正常\n旧内容')
add('docx','floating',doc(p('正常')+'<w:p><w:r><w:drawing><wp:anchor>'+p(EVENT)+'</wp:anchor></w:drawing></w:r></w:p>'),'正常','PartialParse',gold='正常\n'+EVENT)
add('docx','textbox',doc(p('正常')+'<w:txbxContent>'+p(EVENT)+'</w:txbxContent>'),'正常','PartialParse',gold='正常\n'+EVENT)
add('docx','columns',doc(p(EVENT)+'<w:sectPr><w:cols w:num="2"/></w:sectPr>'),EVENT,'PartialParse')
add('docx','object',doc(p('正常')+'<w:object/>'),'正常','PartialParse')
add('docx','field',doc(p('正常')+'<w:p><w:r><w:instrText>DATE</w:instrText></w:r></w:p>'),'正常','PartialParse')
add('docx','hyperlink',doc('<w:p><w:hyperlink r:id="h"><w:r><w:t>'+EVENT+'</w:t></w:r></w:hyperlink></w:p>',{'word/_rels/document.xml.rels':rels([('h','hyperlink','https://invalid.example',True)])}),EVENT,'PartialParse')
add('docx','wrong_namespace',doc(p(EVENT),ns='urn:spoof'),'','Unsupported',gold=EVENT)
add('docx','dtd',doc(p(EVENT))|{'word/document.xml':'<!DOCTYPE document [<!ENTITY x SYSTEM "file:///never-read">]><w:document xmlns:w="'+W+'"><w:body>'+p(EVENT)+'</w:body></w:document>'},'','Unsupported',gold=EVENT)
add('docx','macro',doc(p(EVENT),main_type='application/vnd.ms-word.document.macroEnabled.main+xml'),'','Unsupported',gold=EVENT)
add('docx','renamed_prefix',{n:b.replace('w:','z:').replace('xmlns:w=','xmlns:z=') if n=='word/document.xml' else b for n,b in doc(p(EVENT)).items()},EVENT)
add('docx','headers_omitted',doc(p('正文'),{'word/header1.xml':'<w:hdr xmlns:w="'+W+'">'+p(EVENT)+'</w:hdr>','word/_rels/document.xml.rels':rels([('h','header','header1.xml',False)])}),'正文','PartialParse',gold='正文\n'+EVENT)
add('docx','line_break',doc('<w:p><w:r><w:t>第一行</w:t><w:br/><w:t>第二行</w:t></w:r></w:p>'),'第一行\n第二行')
add('docx','nested_table',doc('<w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc>'+p(EVENT)+'</w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>'),'','PartialParse',gold=EVENT)
add('docx','hidden_run',doc('<w:p><w:r><w:rPr><w:vanish/></w:rPr><w:t>'+EVENT+'</w:t></w:r></w:p>'),'','PartialParse',gold=EVENT)
for name,t in [('visible',EVENT),('unicode','欢迎📚'),('escaped','A&B <通知>'),('negated','明天不考试'),('question','明天考试吗？')]:add('xlsx',name,book(row(cell(t))),t)
add('xlsx','yearless',book(row(cell('10月12日高数考试'))),'10月12日高数考试',event_gold=[{'title':'高数考试','precision':'UnknownDate','local_date':None}])
add('xlsx','dateonly',book(row(cell('2026年10月12日高数考试'))),'2026年10月12日高数考试',event_gold=[{'title':'高数考试','precision':'DateOnly','local_date':'2026-10-12'}])
add('xlsx','coordinates',book(row(cell('通知','B1'))),'通知')
add('xlsx','hidden_row',book(row(cell(EVENT),'hidden="1"')),'','PartialParse',gold=EVENT)
add('xlsx','hidden_column',book(row(cell(EVENT)),sheet_extra='<cols><col min="1" max="1" hidden="1"/></cols>'),'','PartialParse',gold=EVENT)
add('xlsx','hidden_sheet',book(row(cell(EVENT)),sheet_attrs='state="hidden"'),'','PartialParse',gold=EVENT)
add('xlsx','formula',book(row('<c r="A1" t="str"><f>CONCAT()</f><v>'+EVENT+'</v></c>')),EVENT,'PartialParse')
add('xlsx','formula_date',book(row('<c r="A1" s="0"><f>TODAY()</f><v>46307</v></c>'),style='<cellXfs><xf numFmtId="14"/></cellXfs>'),'2026-10-12','PartialParse',event_gold=[])
add('xlsx','date1900',book(row(cell('46307',kind='n',style=0)),style='<cellXfs><xf numFmtId="14"/></cellXfs>'),'2026-10-12')
add('xlsx','date1904',book(row(cell('44845',kind='n',style=0)),date1904=True,style='<cellXfs><xf numFmtId="14"/></cellXfs>'),'2026-10-12')
add('xlsx','serial60',book(row(cell('60',kind='n',style=0)),style='<cellXfs><xf numFmtId="14"/></cellXfs>'),'60','PartialParse')
add('xlsx','numeric',book(row(cell('46307',kind='n'))),'46307','PartialParse')
add('xlsx','custom_format',book(row(cell('46307',kind='n',style=0)),style='<numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><cellXfs><xf numFmtId="164"/></cellXfs>'),'2026-10-12')
add('xlsx','ambiguous_format',book(row(cell('46307',kind='n',style=0)),style='<numFmts><numFmt numFmtId="164" formatCode="mm:ss"/></numFmts><cellXfs><xf numFmtId="164"/></cellXfs>'),'46307','PartialParse')
merged=book(row(cell(EVENT)));merged['xl/worksheets/sheet1.xml']=merged['xl/worksheets/sheet1.xml'].replace('</worksheet>','<mergeCells><mergeCell ref="A1:B1"/></mergeCells></worksheet>');add('xlsx','merged',merged,EVENT,'PartialParse')
shared=book(row(cell('0',kind='s')));shared['xl/sharedStrings.xml']='<sst xmlns="'+S+'"><si><r><t>2026年10月12日</t></r><r><t>9:00高数考试</t></r></si></sst>';shared['xl/_rels/workbook.xml.rels']=rels([('sheet','worksheet','worksheets/sheet1.xml',False),('ss','sharedStrings','sharedStrings.xml',False)]);shared['[Content_Types].xml']=types('xl/workbook.xml',X,[('xl/worksheets/sheet1.xml','application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml'),('xl/sharedStrings.xml','application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml')]);add('xlsx','shared_strings',shared,EVENT)
external=book(row(cell(EVENT)));external['xl/_rels/workbook.xml.rels']=rels([('sheet','worksheet','worksheets/sheet1.xml',False),('ext','externalLink','https://invalid.example',True)]);add('xlsx','external',external,EVENT,'PartialParse')
macro=book(row(cell(EVENT)));macro['[Content_Types].xml']=types('xl/workbook.xml','application/vnd.ms-excel.sheet.macroEnabled.main+xml');add('xlsx','macro',macro,'','Unsupported',gold=EVENT)
wrong=book(row(cell(EVENT)));wrong['xl/worksheets/sheet1.xml']=wrong['xl/worksheets/sheet1.xml'].replace(S,'urn:spoof');add('xlsx','wrong_namespace',wrong,'','Unsupported',gold=EVENT)
dtd=book(row(cell(EVENT)));dtd['xl/worksheets/sheet1.xml']='<!DOCTYPE worksheet [<!ENTITY x SYSTEM "https://invalid.example">]>'+dtd['xl/worksheets/sheet1.xml'];add('xlsx','dtd',dtd,'','Unsupported',gold=EVENT)
for fmt,cases in CASES.items():
 (ROOT/(fmt+'_manifest.json')).write_text(json.dumps(dict(schema_version=1,synthetic_only=True,fixtures=cases),ensure_ascii=False,indent=2)+'\n')
