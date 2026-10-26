import wave,array,sys
w=wave.open(sys.argv[1]); r=w.getframerate(); a=array.array('h',w.readframes(w.getnframes()))
win=r//100; run=0
for i in range(0,len(a),win):
    if max(abs(x) for x in a[i:i+win])>500:
        if run>=70: print('gap %.2f -> %.2f' % (i/r-run/100, i/r))
        run=0
    else: run+=1
