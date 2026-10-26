import wave,array,sys
def load(p):
    if p.endswith('.pcm'): return array.array('h',open(p,'rb').read()),24000
    w=wave.open(p); return array.array('h',w.readframes(w.getnframes())),w.getframerate()
for p in sys.argv[1:]:
    a,r=load(p)
    win=r//100; run=0; sil=[]
    for i in range(0,len(a),win):
        if max(abs(x) for x in a[i:i+win])>500:
            if run: sil.append(run/100); run=0
        else: run+=1
    # dropouts: exact-zero runs >=5ms that sit between loud audio (inside speech)
    drops=[];z=0
    for i,x in enumerate(a):
        if x==0: z+=1
        else:
            if z>=r*0.005 and i-z>0 and max(abs(y) for y in a[max(0,i-z-240):i-z] or [0])>300: drops.append((round((i-z)/r,2),round(z/r*1000)))
            z=0
    print(p, f'{len(a)/r:.1f}s', 'longest silences', sorted(sil)[-4:], 'mid-speech dropouts', drops[:20])
