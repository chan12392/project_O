/** The slice of three the graph canvas uses; the package ships no type declarations. */

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface ThreeAttribute {
  array: Float32Array;
  needsUpdate: boolean;
}

export interface ThreeGeometry {
  setAttribute(name: string, attribute: ThreeAttribute): void;
  setDrawRange(start: number, count: number): void;
  dispose(): void;
}

export interface ThreeMaterial {
  blending: number;
  needsUpdate: boolean;
  uniforms: Record<string, { value: number }>;
  dispose(): void;
}

export interface ThreeObject {
  frustumCulled: boolean;
  visible: boolean;
}

export interface ThreeInstanced extends ThreeObject {
  count: number;
  instanceMatrix: ThreeAttribute;
  instanceColor: ThreeAttribute | null;
  dispose(): void;
}

export interface ThreeScene {
  add(...objects: ThreeObject[]): void;
  remove(...objects: ThreeObject[]): void;
}

export interface ThreeCamera {
  fov: number;
  aspect: number;
  position: Vec3;
}

export interface Three {
  SphereGeometry: new (radius: number, widthSegments: number, heightSegments: number) => ThreeGeometry;
  BufferGeometry: new () => ThreeGeometry;
  BufferAttribute: new (array: Float32Array, itemSize: number) => ThreeAttribute;
  InstancedBufferAttribute: new (array: Float32Array, itemSize: number) => ThreeAttribute;
  InstancedMesh: new (geometry: ThreeGeometry, material: ThreeMaterial, count: number) => ThreeInstanced;
  LineSegments: new (geometry: ThreeGeometry, material: ThreeMaterial) => ThreeObject;
  Points: new (geometry: ThreeGeometry, material: ThreeMaterial) => ThreeObject;
  MeshBasicMaterial: new (params?: { color?: string }) => ThreeMaterial;
  LineBasicMaterial: new (params: { vertexColors: boolean }) => ThreeMaterial;
  ShaderMaterial: new (params: {
    uniforms: Record<string, { value: number }>;
    vertexShader: string;
    fragmentShader: string;
    transparent: boolean;
    depthWrite: boolean;
    blending: number;
  }) => ThreeMaterial;
  /** r, g, b in three's linear working space, read from any color three can parse (hex here). */
  Color: new (color: string) => { r: number; g: number; b: number };
  Vector2: new (x: number, y: number) => unknown;
  Raycaster: new () => { setFromCamera(pointer: unknown, camera: unknown): void; ray: { origin: Vec3; direction: Vec3 } };
  NormalBlending: number;
  AdditiveBlending: number;
}
