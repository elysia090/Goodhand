#!/usr/bin/env python3
"""Pack neutral 2D occupancy as independent row runs; never bake tone into shape."""
import argparse, hashlib, json, pathlib, struct
import numpy as np
HEADER=struct.Struct("<4sHHHHI")
def encode(mask, fps=24):
    mask=np.asarray(mask,dtype=np.bool_)
    if mask.ndim != 3: raise ValueError("Expected frame,row,column occupancy")
    frames,height,width=mask.shape
    if not (0<frames<=4096 and 0<height<=255 and 0<width<=255 and 0<fps<=1000):
        raise ValueError("Unsupported dimensions or clock")
    offsets=[0]; payload=bytearray()
    for frame in mask:
        for row in frame:
            edges=np.flatnonzero(np.diff(np.r_[False,row,False].astype(np.int8)))
            runs=edges.reshape(-1,2)
            payload.append(len(runs))
            for start,end in runs: payload.extend((int(start),int(end-start)))
        offsets.append(len(payload))
    return HEADER.pack(b"GHSH",1,width,height,fps,frames)+struct.pack("<"+"I"*len(offsets),*offsets)+payload

def decode(blob):
    if len(blob)<HEADER.size: raise ValueError("Truncated header")
    magic,version,width,height,fps,frames=HEADER.unpack_from(blob)
    if magic!=b"GHSH" or version!=1: raise ValueError("Unknown shape format")
    if not(0<width<=255 and 0<height<=255 and 0<fps<=1000 and 0<frames<=4096):
        raise ValueError("Invalid dimensions or clock")
    start=HEADER.size+4*(frames+1)
    if len(blob)<start: raise ValueError("Truncated offsets")
    offsets=struct.unpack_from("<"+"I"*(frames+1),blob,HEADER.size)
    if offsets[0]!=0 or offsets[-1]!=len(blob)-start: raise ValueError("Invalid extent")
    mask=np.zeros((frames,height,width),dtype=np.bool_)
    for index,(left,right) in enumerate(zip(offsets,offsets[1:])):
        if not 0<=left<=right<=len(blob)-start: raise ValueError("Invalid offset")
        pos=start+left; end=start+right
        for y in range(height):
            if pos>=end: raise ValueError("Truncated row")
            count=blob[pos];pos+=1; previous=-1
            for _ in range(count):
                if pos+2>end: raise ValueError("Truncated run")
                x,length=blob[pos:pos+2];pos+=2
                if not length or x<=previous or x+length>width: raise ValueError("Noncanonical run")
                mask[index,y,x:x+length]=True; previous=x+length
        if pos!=end: raise ValueError("Trailing frame data")
    return mask,fps

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observations",type=pathlib.Path)
    parser.add_argument("destination",type=pathlib.Path)
    args=parser.parse_args()
    if args.destination.exists() or args.destination.with_suffix(".json").exists():
        raise FileExistsError("Refusing to overwrite existing shape or report")
    source=np.load(args.observations)["mask"]
    blob=encode(source)
    recovered,fps=decode(blob)
    if not np.array_equal(source,recovered): raise RuntimeError("Shape round-trip changed occupancy")
    args.destination.parent.mkdir(parents=True,exist_ok=True)
    with args.destination.open("xb") as target: target.write(blob)
    report={"format":"GHSH v1 independent row runs","shape":[int(x) for x in source.shape],
      "fps":fps,"bytes":len(blob),"bitpacked_comparison_bytes":int(np.packbits(source).nbytes),
      "round_trip_differing_cells":int(np.count_nonzero(source!=recovered)),
      "sha256":hashlib.sha256(blob).hexdigest(),
      "meaning":"2D occupancy only; no tone, depth, normals, or model topology"}
    with args.destination.with_suffix(".json").open("x") as target:json.dump(report,target,indent=2);target.write("\n")
    print(json.dumps(report,indent=2))
