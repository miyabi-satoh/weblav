import { describe, expect, it } from 'vitest';
import { videoEmbedUrl } from './video-embed';

const EMBED = 'https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ';

describe('videoEmbedUrl', () => {
	it('YouTube の動画を指す URL の形を、どれも埋め込みの URL にする', () => {
		for (const url of [
			'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
			'https://m.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123',
			'http://youtube.com/watch?feature=share&v=dQw4w9WgXcQ',
			'https://youtu.be/dQw4w9WgXcQ?si=abc',
			'https://www.youtube.com/shorts/dQw4w9WgXcQ',
			'https://www.youtube.com/live/dQw4w9WgXcQ',
			'https://www.youtube.com/embed/dQw4w9WgXcQ',
			'https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ'
		]) {
			expect(videoEmbedUrl(url), url).toBe(EMBED);
		}
	});

	it('始める位置を秒にして渡す', () => {
		expect(videoEmbedUrl('https://youtu.be/dQw4w9WgXcQ?t=90')).toBe(`${EMBED}?start=90`);
		expect(videoEmbedUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1m30s')).toBe(
			`${EMBED}?start=90`
		);
		expect(videoEmbedUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1h2m3s')).toBe(
			`${EMBED}?start=3723`
		);
		expect(videoEmbedUrl('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=abc')).toBe(EMBED);
	});

	it('動画を指さない URL と、ほかのサイトはリンクのカードのままにする', () => {
		for (const url of [
			'https://www.youtube.com/',
			'https://www.youtube.com/@nhk',
			'https://www.youtube.com/playlist?list=PL123',
			'https://www.youtube.com/watch?v=short',
			'https://www.youtube.com/watch/dQw4w9WgXcQ',
			'https://notyoutube.com/watch?v=dQw4w9WgXcQ',
			'https://vimeo.com/76979871',
			'not a url'
		]) {
			expect(videoEmbedUrl(url), url).toBeUndefined();
		}
	});
});
