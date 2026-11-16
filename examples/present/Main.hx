import hxn.gpu.gles.*;
import hxn.window.Window;
import hxn.window.WindowAttributes;

/**
	Presents to a window: a context on an hxn-window window on Xlib, three
	frames each cleared to one color and swapped, the last read back first
	and checked.
**/
class Main {
	/** `EGL_PLATFORM_X11_KHR`. **/
	static inline final PLATFORM_X11 = 0x31D5;

	static function main() {
		var attributes = new WindowAttributes();
		attributes.title("hxn-gpu-gles present");
		attributes.width(64);
		attributes.height(64);
		var window = Window.open(attributes);
		attributes.close();
		if (window.platform() != 3)
			throw 'the window is on platform ${window.platform()}, not Xlib';

		// Xlib's raw handles: the window, its visual id and the `Display*`.
		var display = Display.native(PLATFORM_X11, window.raw(2));
		var context = display.createWindowContext(window.raw(0), haxe.Int64.toInt(window.raw(1)));
		context.swapInterval(0);
		var size = 64;
		var colors = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
		for (frame in 0...colors.length) {
			var c = colors[frame];
			context.bindDefaultFramebuffer();
			context.viewport(0, 0, size, size);
			context.clear(ClearBits.Color, c[0], c[1], c[2], 1);
			if (frame == colors.length - 1) {
				var pixel = context.readPixels(0, 0, 1, 1);
				var expected = [for (v in c) Math.round(v * 255)];
				for (i in 0...3)
					if (pixel.get(i) != expected[i])
						throw 'the last frame is ${pixel.get(0)}, ${pixel.get(1)}, ${pixel.get(2)}, not $expected';
				Sys.println('present: ${colors.length} frames to a ${size}x$size window, the last ${expected.join(", ")}');
			}
			context.swapBuffers();
		}
		var error = context.error();
		if (error != 0)
			throw 'OpenGL ES error 0x${StringTools.hex(error)}';
		context.close();
		display.close();
		window.close();
	}
}
