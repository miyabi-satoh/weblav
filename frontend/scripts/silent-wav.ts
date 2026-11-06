/** 無音の WAV (8kHz・8bit・モノラル)。 */
export function silentWav(seconds: number): Buffer {
	const samples = 8000 * seconds;
	const buffer = Buffer.alloc(44 + samples, 128);
	buffer.write('RIFF', 0);
	buffer.writeUInt32LE(36 + samples, 4);
	buffer.write('WAVEfmt ', 8);
	buffer.writeUInt32LE(16, 16);
	buffer.writeUInt16LE(1, 20);
	buffer.writeUInt16LE(1, 22);
	buffer.writeUInt32LE(8000, 24);
	buffer.writeUInt32LE(8000, 28);
	buffer.writeUInt16LE(1, 32);
	buffer.writeUInt16LE(8, 34);
	buffer.write('data', 36);
	buffer.writeUInt32LE(samples, 40);
	return buffer;
}
