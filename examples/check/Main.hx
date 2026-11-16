import hxn.gpu.gles.*;
import haxe.io.Bytes;
import Shaders;

/**
	What CI runs: a render pass into a texture, read back once the GPU has
	finished; a pass sampling a texture; and a buffer read back through a
	mapping. It needs EGL and OpenGL ES 3.0, and prints the same lines on
	every runtime.
**/
class Main {
	static function check(ok:Bool, message:String) {
		if (!ok)
			throw message;
	}

	static function closeAll(objects:Array<{function close():Void;}>) {
		for (o in objects)
			o.close();
	}

	static function main() {
		var display = try Display.headless() catch (e:haxe.Exception) {
			Sys.println('no EGL: ${e.message}');
			return;
		}
		var context = try display.createContext() catch (e:haxe.Exception) {
			Sys.println('no OpenGL ES 3: ${e.message}');
			return;
		}
		// The render pass completes in a callback from the Haxe event loop, after main
		// returns, and the other passes run from there.
		render(context, () -> {
			textured(context);
			buffer(context);
			context.close();
			display.close();
		});
		Sys.println("render: submitted");
	}

	/** A 4x4 Rgba8 texture and a framebuffer rendering into it, bound and cleared. **/
	static function target(context:Context):{texture:Texture, framebuffer:Framebuffer} {
		var texture = context.createTexture(4, 4, TextureFormat.Rgba8);
		var framebuffer = context.createFramebuffer(texture);
		context.bindFramebuffer(framebuffer);
		context.viewport(0, 0, 4, 4);
		context.clear(ClearBits.Color, 0, 0, 0, 0);
		return {texture: texture, framebuffer: framebuffer};
	}

	/** Reads the bound 4x4 framebuffer, and checks every pixel is `expected`. **/
	static function readTarget(context:Context, expected:Array<Int>, label:String) {
		var pixels = context.readPixels(0, 0, 4, 4);
		for (i in 0...16)
			for (c in 0...4)
				check(pixels.get(i * 4 + c) == expected[c], '$label: pixel $i channel $c is ${pixels.get(i * 4 + c)}, not ${expected[c]}');
		Sys.println('$label: 4x4 pixels of ${[for (c in 0...4) pixels.get(c)].join(", ")}');
	}

	static function quad(context:Context, values:Array<Float>):Buffer {
		var bytes = Bytes.alloc(values.length * 4);
		for (i in 0...values.length)
			bytes.setFloat(i * 4, values[i]);
		return context.createBufferWith(BufferTarget.Array, bytes, Usage.StaticDraw);
	}

	static function render(context:Context, next:() -> Void) {
		var bindings = new Bindings(Tinted.GLSL_BINDINGS);
		var program = hxn.gpu.gles.Shaders.program(context, Tinted.GLSL_VERTEX, Tinted.GLSL_FRAGMENT, bindings);
		var positions = quad(context, [-1, -1, 1, -1, -1, 1, 1, 1]);
		var vertices = context.createVertexArray();
		vertices.attribute(Tinted.INPUT_position, positions, 2, ComponentType.Float, false, 8, 0);
		var params = Bytes.alloc(Tinted.PARAMS_SIZE);
		var tint = [0.25, 0.5, 0.75, 1.0];
		for (i in 0...4)
			params.setFloat(Tinted.PARAMS_tint + i * 4, tint[i]);
		var uniforms = context.createBufferWith(BufferTarget.Uniform, params, Usage.StaticDraw);

		var t = target(context);
		context.useProgram(program);
		context.bindVertexArray(vertices);
		context.bindUniformBuffer(bindings.uniformBlock(Tinted.PARAMS_GROUP, Tinted.PARAMS_BINDING), uniforms);
		context.drawArrays(Primitive.TriangleStrip, 0, 4, 1);
		context.completed().then(_ -> {
			readTarget(context, [for (c in tint) Math.round(c * 255)], "render");
			closeAll([t.framebuffer, t.texture, uniforms, vertices, positions, program]);
			next();
		}, e -> {
			Sys.println('render failed: ${e.message}');
			hxn.Loop.exit(1);
		});
	}

	static function textured(context:Context) {
		var bindings = new Bindings(Textured.GLSL_BINDINGS);
		var program = hxn.gpu.gles.Shaders.program(context, Textured.GLSL_VERTEX, Textured.GLSL_FRAGMENT, bindings);
		var corners = quad(context, [-1, -1, 0, 1, 1, -1, 1, 1, -1, 1, 0, 0, 1, 1, 1, 0]);
		var vertices = context.createVertexArray();
		vertices.attribute(Textured.INPUT_position, corners, 2, ComponentType.Float, false, 16, 0);
		vertices.attribute(Textured.INPUT_uv, corners, 2, ComponentType.Float, false, 16, 8);

		// A 2x2 image of one color.
		var color = [40, 80, 120, 255];
		var image = context.createTexture(2, 2, TextureFormat.Rgba8);
		var pixels = Bytes.alloc(2 * 2 * 4);
		for (i in 0...4)
			for (c in 0...4)
				pixels.set(i * 4 + c, color[c]);
		image.write(0, 0, 2, 2, pixels);
		var sampler = context.createSampler(Filter.Nearest, Filter.Nearest, Wrap.ClampToEdge, Wrap.ClampToEdge);

		var t = target(context);
		context.useProgram(program);
		context.bindVertexArray(vertices);
		context.bindTexture(bindings.textureUnit(Textured.TEXTURE_image_GROUP, Textured.TEXTURE_image), image, sampler);
		context.drawArrays(Primitive.TriangleStrip, 0, 4, 1);
		readTarget(context, color, "textured");
		closeAll([t.framebuffer, t.texture, sampler, image, vertices, corners, program]);
	}

	static function buffer(context:Context) {
		var count = 16;
		var values = Bytes.alloc(count * 4);
		for (i in 0...count)
			values.setInt32(i * 4, i * 7);
		var data = context.createBuffer(BufferTarget.CopyRead, count * 4, Usage.StaticRead);
		data.write(0, values);
		var read = data.read(0, count * 4);
		for (i in 0...count)
			check(read.getInt32(i * 4) == i * 7, 'value $i is ${read.getInt32(i * 4)}, not ${i * 7}');
		Sys.println('buffer: $count values written and read back, the last ${read.getInt32((count - 1) * 4)}');
		data.close();
		var error = context.error();
		check(error == 0, 'OpenGL ES error 0x${StringTools.hex(error)}');
	}
}
