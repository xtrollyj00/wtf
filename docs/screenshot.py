# Turns `wtf --snapshot --ansi` output into an HTML page, for docs/demo.png:
#   wtf docs/demo.vcd --snapshot 120 --ansi | python3 docs/screenshot.py > /tmp/demo.html
#   firefox --headless --screenshot "$PWD/docs/demo.png" --window-size=1380,742 file:///tmp/demo.html
# Needs the Noto Sans Mono font; the legacy computing diagonals of bus edges are drawn as SVG,
# as fonts rarely have them. The page is shown at 1.25 times its size.
import re, sys, html
def c256(n):
    n=int(n)
    base=[(0,0,0),(205,49,49),(13,188,121),(229,229,16),(36,114,200),(188,63,188),(17,168,205),(229,229,229),(102,102,102),(241,76,76),(35,209,139),(245,245,67),(59,142,234),(214,112,214),(41,184,219),(255,255,255)]
    if n<16: return '#%02x%02x%02x'%base[n]
    if n<232:
        n-=16; v=[0,95,135,175,215,255]; return '#%02x%02x%02x'%(v[n//36],v[n//6%6],v[n%6])
    g=8+(n-232)*10; return '#%02x%02x%02x'%(g,g,g)
BG,FG='#1e2127','#abb2bf'
W,H=9,20
# legacy computing diagonals (U+1FBA0..): points as (x,y) in a 0..2 grid
D={0x1FBA0:[(1,0),(0,1)],0x1FBA1:[(1,0),(2,1)],0x1FBA2:[(0,1),(1,2)],0x1FBA3:[(2,1),(1,2)],
0x1FBA4:[(1,0),(0,1),(1,2)],0x1FBA5:[(1,0),(2,1),(1,2)],0x1FBA6:[(0,1),(1,2),(2,1)],0x1FBA7:[(0,1),(1,0),(2,1)]}
def cell(ch,s,fg):
    o=ord(ch)
    if o in D:
        pts=' '.join('%g,%g'%(x*W/2,y*H/2) for x,y in D[o])
        inner='<svg width="%d" height="%d" style="display:block"><polyline points="%s" fill="none" stroke="%s" stroke-width="1.2"/></svg>'%(W,H,pts,fg)
    else: inner=html.escape(ch)
    return '<span style="%sdisplay:inline-block;width:%dpx;height:%dpx;line-height:%dpx;overflow:visible;vertical-align:top">%s</span>'%(s,W,H,H,inner)
out=[]
for line in sys.stdin.read().split('\n'):
    st={'fg':None,'bg':None,'b':False,'r':False,'i':False,'u':False}
    parts=re.split(r'(\x1b\[[0-9;:]*m)',line); row=''
    for p in parts:
        m=re.match(r'\x1b\[([0-9;:]*)m',p)
        if m:
            a=[x for x in re.split('[;:]',m.group(1))] or ['0']
            i=0
            while i<len(a):
                x=a[i] or '0'; x=int(x)
                if x==0: st={'fg':None,'bg':None,'b':False,'r':False,'i':False,'u':False}
                elif x==1: st['b']=True
                elif x==3: st['i']=True
                elif x==4: st['u']=True
                elif x==7: st['r']=True
                elif x==22: st['b']=False
                elif x==23: st['i']=False
                elif x==24: st['u']=False
                elif x==27: st['r']=False
                elif x in (38,48):
                    k='fg' if x==38 else 'bg'
                    if a[i+1]=='2': st[k]='#%02x%02x%02x'%tuple(int(v) for v in a[i+2:i+5]); i+=4
                    else: st[k]=c256(a[i+2]); i+=2
                elif x==39: st['fg']=None
                elif x==49: st['bg']=None
                elif 30<=x<=37: st['fg']=c256(x-30)
                elif 90<=x<=97: st['fg']=c256(x-90+8)
                elif 40<=x<=47: st['bg']=c256(x-40)
                i+=1
            continue
        if not p: continue
        fg,bg=st['fg'] or FG, st['bg'] or BG
        if st['r']: fg,bg=bg,fg
        s='color:%s;background:%s;'%(fg,bg)+('font-weight:bold;' if st['b'] else '')+('font-style:italic;' if st['i'] else '')+('text-decoration:underline;' if st['u'] else '')
        for ch in p: row+=cell(ch,s,fg)
    out.append(row)
print('<html><body style="zoom:1.25;margin:0;background:%s"><div style="margin:8px;font:15px \'Noto Sans Mono\',monospace;color:%s;white-space:pre">%s</div></body></html>'%(BG,FG,''.join('<div style="height:%dpx">%s</div>'%(H,r) for r in out)))
