package hxn.gpu.gles;

#if hxnsl
/**
	An hxnsl shader for OpenGL ES. It gets hxnsl's `GLSL_VERTEX`,
	`GLSL_FRAGMENT` and `GLSL_BINDINGS`, or `GLSL_MISSING` for a shader
	OpenGL ES 3.0 cannot run; `Bindings` assigns its binding points and
	texture units.
**/
interface Shader extends hxnsl.Shader {}
#end
