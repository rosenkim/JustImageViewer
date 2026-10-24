"""Build small, original GLB fixtures without third-party assets or tools."""
import json
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).parent

def png(color):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>2I5B', 1, 1, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(b'\0' + bytes(color))) + chunk(b'IEND', b'')

def fixture():
    data = bytearray()
    views = []
    accessors = []
    def view(raw):
        while len(data) % 4: data.append(0)
        i = len(views)
        views.append(dict(buffer=0, byteOffset=len(data), byteLength=len(raw)))
        data.extend(raw)
        return i
    def accessor(values, size, kind, component=5126, **extra):
        flat = [v for row in values for v in row]
        fmt = 'f' if component == 5126 else 'H'
        i = len(accessors)
        accessors.append(dict(bufferView=view(struct.pack('<' + fmt * len(flat), *flat)), componentType=component, count=len(values), type=kind, **extra))
        return i
    positions = [(-1, -1, 0), (1, -1, 0), (0, 1, 0)]
    pos = accessor(positions, 3, 'VEC3', min=[-1,-1,0], max=[1,1,0])
    uv = accessor([(0,0), (1,0), (0.5,1)], 2, 'VEC2')
    norm = accessor([(0,0,1)]*3, 3, 'VEC3')
    images = []
    for color in [(220,70,40,255), (255,160,70,255), (128,128,255,255), (180,180,180,255), (30,10,5,255)]:
        images.append(dict(bufferView=view(png(color)), mimeType='image/png'))
    doc = dict(asset=dict(version='2.0', generator='JustImageViewer test fixtures'), scene=0, scenes=[dict(nodes=[0])],
        nodes=[dict(mesh=0)], meshes=[dict(primitives=[dict(attributes=dict(POSITION=pos,NORMAL=norm,TEXCOORD_0=uv), material=0)])],
        images=images, textures=[dict(source=i) for i in range(5)],
        materials=[dict(pbrMetallicRoughness=dict(baseColorTexture=dict(index=0), metallicRoughnessTexture=dict(index=1), metallicFactor=0.4, roughnessFactor=0.7),
            normalTexture=dict(index=2), occlusionTexture=dict(index=3), emissiveTexture=dict(index=4), emissiveFactor=[0.1,0.1,0.1], doubleSided=True)],
        bufferViews=views, accessors=accessors)
    return doc, data, accessor

def save(name, doc, data):
    doc['buffers'] = [dict(byteLength=len(data))]
    raw = json.dumps(doc, separators=(',', ':')).encode()
    raw += b' ' * (-len(raw) % 4)
    data += b'\0' * (-len(data) % 4)
    body = struct.pack('<I4s',len(raw),b'JSON') + raw + struct.pack('<I4s',len(data),b'BIN\0') + data
    (ROOT/name).write_bytes(struct.pack('<4sII',b'glTF',2,12+len(body)) + body)

if __name__ == '__main__':
    (ROOT/'image.png').write_bytes(png((40,180,220,255)))
    doc, data, acc = fixture()
    save('textured.glb', doc, data)
    doc, data, acc = fixture()
    doc['nodes'] = [dict(translation=[2,3,4],children=[1]), dict(mesh=0,scale=[2,2,2])]
    save('transformed.glb',doc,data)
    doc, data, acc = fixture()
    delta = acc([(0,0,1)]*3,3,'VEC3', min=[0,0,1], max=[0,0,1])
    doc['meshes'][0]['primitives'][0]['targets'] = [dict(POSITION=delta)]
    doc['meshes'][0]['weights'] = [0.5]
    save('morph.glb',doc,data)
    doc, data, acc = fixture()
    joints = acc([(0,0,0,0)]*3,4,'VEC4',5123)
    weights = acc([(1,0,0,0)]*3,4,'VEC4')
    doc['meshes'][0]['primitives'][0]['attributes'].update(JOINTS_0=joints, WEIGHTS_0=weights)
    doc['nodes'] = [dict(mesh=0, skin=0),dict(translation=[0,2,0])]
    doc['scenes'][0]['nodes'] = [0,1]
    doc['skins'] = [dict(joints=[1])]
    save('skin.glb',doc,data)
    doc, data, acc = fixture()
    doc['materials'][0]['alphaMode'] = 'BLEND'
    doc['materials'][0]['pbrMetallicRoughness']['baseColorFactor'] = [1,1,1,0.4]
    save('transparent.glb',doc,data)
    # A cube gives the native viewer a useful orbit target.
    doc, data, acc = fixture()
    positions = []
    normals = []
    uvs = []
    faces = [((0,0,1), [(-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]),
        ((0,0,-1), [(1,-1,-1),(-1,-1,-1),(-1,1,-1),(1,1,-1)]),
        ((1,0,0), [(1,-1,1),(1,-1,-1),(1,1,-1),(1,1,1)]),
        ((-1,0,0), [(-1,-1,-1),(-1,-1,1),(-1,1,1),(-1,1,-1)]),
        ((0,1,0), [(-1,1,1),(1,1,1),(1,1,-1),(-1,1,-1)]),
        ((0,-1,0), [(-1,-1,-1),(1,-1,-1),(1,-1,1),(-1,-1,1)])]
    for normal, points in faces:
        for i in [0,1,2,0,2,3]:
            positions.append(points[i]); normals.append(normal); uvs.append([(0,0),(1,0),(1,1),(0,1)][i])
    primitive = doc['meshes'][0]['primitives'][0]
    primitive['attributes'] = dict(POSITION=acc(positions,3,'VEC3',min=[-1,-1,-1],max=[1,1,1]), NORMAL=acc(normals,3,'VEC3'), TEXCOORD_0=acc(uvs,2,'VEC2'))
    save('cube.glb',doc,data)
