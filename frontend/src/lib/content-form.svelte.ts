/**
 * コンテンツの作成・編集フォームが持つ値。
 *
 * 作成 (ダイアログ) と編集 (ページ) で同じ項目を扱うため、値の持ち方をここに集める。
 * `ErrorDialogState` と同じく、画面側は1つのインスタンスを渡すだけでよい。
 *
 * 親グループによる実際の見え方は、ほかの行を見ないと決まらない。
 * `contentFormVisibilityNote` に一覧を渡し、画面側の `$derived` で呼ぶ。
 */
import { client } from '$lib/api/client';
import type { components } from '$lib/api/schema';
import { visibilityLabel } from '$lib/content-labels';
import { isDirectoryContentType } from '$lib/content-types';
import { FormDirtyState } from '$lib/form-dirty.svelte';
import { contentPathLabel } from '$lib/root-location';
import { effectiveVisibilityUnder } from '$lib/visibility';
import * as m from '$lib/paraglide/messages.js';

type Content = components['schemas']['AdminContentResponse'];
type ContentType = components['schemas']['ContentType'];
type Visibility = components['schemas']['Visibility'];
type LinkMetadataResponse = components['schemas']['LinkMetadataResponse'];

/** `POST /contents/link-metadata` を呼ぶ。取れない・通信に失敗したら `null`。 */
async function fetchLinkMetadata(url: string): Promise<LinkMetadataResponse | null> {
	try {
		const { data, response } = await client.POST('/api/v1/contents/link-metadata', {
			body: { url }
		});
		return response.ok && data ? data : null;
	} catch {
		return null;
	}
}

/** 開いた時点との比較に使う値。`FileList` は比較できないので、選ばれているかだけを見る。 */
type FormSnapshot = {
	type: ContentType;
	visibility: Visibility;
	title: string;
	url: string;
	path: string;
	parentId: string;
	description: string;
	hasFiles: boolean;
	extensions: string;
};

export class ContentFormState {
	/** `type` は更新できない (サーバーも更新リクエストで受け付けない)。編集時は読み取り専用で持つ。 */
	type = $state<ContentType>('link');
	title = $state('');
	url = $state('');
	path = $state('');
	/** `path` の画面での出し方 (→ `$lib/root-location`)。`path` と一緒に変える。 */
	pathLabel = $state('');
	/**
	 * 親グループ。`Select.Root` の制約で値は文字列にする。
	 * `"root"` はルート直下、それ以外は親 group の id を文字列にしたもの。
	 */
	parentId = $state('root');
	description = $state('');
	/** `archive` 専用。対象拡張子 (カンマ区切り、空なら全ファイル)。 */
	extensions = $state('');
	/**
	 * `archive` 専用。表示タイトルの組み立てテンプレート。
	 * この画面では編集させず (軸がまだ無い・変わりうるため、軸の設定画面に委ねる)、
	 * 取得した値を保存時にそのまま送り返すためだけに持つ。
	 */
	titleTemplate = $state<string | null>(null);
	visibility = $state<Visibility>('public');
	/**
	 * `file` 専用。作成時は必須、編集時は「差し替えるときだけ選ぶ」任意項目
	 * (未選択なら既存の blob を保つ)。
	 */
	files = $state<FileList | undefined>(undefined);
	/**
	 * ファイルの入力欄を作り直すための連番。`files` を空にしても、ブラウザは入力欄に
	 * 選んだファイルを残す。保存後もページに留まる編集で、選択済みに見えないようにする。
	 */
	fileInputVersion = $state(0);
	/** 編集時に、差し替えていない現在のファイルを示すための表示用。 */
	existingFileName = $state<string | null>(null);
	existingFileSize = $state<number | null>(null);

	/**
	 * link のタイトル・説明を URL から取り直す (編集フォームの再取得ボタン、→ docs/ui.md「UI 全般」)。
	 * 取得中・直前の取得で何も取れなかったかを、タイトル・説明それぞれ別に持つ。
	 * 失敗しても値は変えない (取得できなかったことだけ伝える)。
	 */
	titleRefetchStatus = $state<'idle' | 'loading' | 'failed'>('idle');
	descriptionRefetchStatus = $state<'idle' | 'loading' | 'failed'>('idle');

	// 最新の再取得だけを反映するための世代番号 (表示には使わないのでリアクティブにしない)。
	// 続けて押されたとき、古い応答が新しい取得の状態を巻き戻さないようにする。
	private titleRefetchGeneration = 0;
	private descriptionRefetchGeneration = 0;

	/**
	 * タイトル・説明の再取得に共通する、世代ガード付きの取得処理。
	 * 両者で同じ手順 (取得中に URL が変わった・追い越された応答を捨てる) を繰り返さないための
	 * 共通処理で、値の当てはめ方だけ呼び出し側 (`applyValue`) に委ねる。
	 */
	private async refetchFromUrl(
		getGeneration: () => number,
		setGeneration: (generation: number) => void,
		setStatus: (status: 'idle' | 'loading' | 'failed') => void,
		applyValue: (metadata: LinkMetadataResponse) => boolean
	): Promise<void> {
		const generation = getGeneration() + 1;
		setGeneration(generation);
		const url = this.url;
		setStatus('loading');
		const metadata = await fetchLinkMetadata(url);
		// より新しい再取得が始まっていれば、この応答は捨てる (状態も触らない)。
		if (generation !== getGeneration()) {
			return;
		}
		// 取得中に URL が変わったら、古い URL の結果は捨てる (現在値を上書きしない)。
		if (this.url !== url) {
			setStatus('idle');
			return;
		}
		setStatus(metadata !== null && applyValue(metadata) ? 'idle' : 'failed');
	}

	async refetchTitleFromUrl(): Promise<void> {
		await this.refetchFromUrl(
			() => this.titleRefetchGeneration,
			(generation) => {
				this.titleRefetchGeneration = generation;
			},
			(status) => {
				this.titleRefetchStatus = status;
			},
			(metadata) => {
				if (!metadata.title) return false;
				this.title = metadata.title;
				return true;
			}
		);
	}

	async refetchDescriptionFromUrl(): Promise<void> {
		await this.refetchFromUrl(
			() => this.descriptionRefetchGeneration,
			(generation) => {
				this.descriptionRefetchGeneration = generation;
			},
			(status) => {
				this.descriptionRefetchStatus = status;
			},
			(metadata) => {
				if (!metadata.description) return false;
				this.description = metadata.description;
				return true;
			}
		);
	}

	/**
	 * 開いた時点の値。バックドロップのクリックと Esc で編集中のフォームを
	 * 誤って閉じないよう、`dirty` の判定に使う。
	 */
	#dirtyState = new FormDirtyState(() => this.snapshot());

	snapshot(): FormSnapshot {
		return {
			type: this.type,
			title: this.title,
			url: this.url,
			path: this.path,
			parentId: this.parentId,
			description: this.description,
			visibility: this.visibility,
			hasFiles: (this.files?.length ?? 0) > 0,
			extensions: this.extensions
		};
	}

	/** 開いた (または保存した) 時点の値を、これ以降の比較の基準にする。 */
	markPristine(): void {
		this.#dirtyState.markPristine();
	}

	/**
	 * 再取得の状態を初期化する (フォームの初期化・再読込時)。世代番号も進めて、
	 * 開始済みの取得の応答が、新しく読み込んだタイトル・説明を上書きしないようにする。
	 */
	private resetRefetchState(): void {
		this.titleRefetchGeneration++;
		this.descriptionRefetchGeneration++;
		this.titleRefetchStatus = 'idle';
		this.descriptionRefetchStatus = 'idle';
	}

	get dirty(): boolean {
		return this.#dirtyState.dirty;
	}

	/** 新規作成の初期値に戻す。`parentId` は「このグループの中に追加」から渡せる。 */
	resetForCreate(parentId = 'root'): void {
		this.type = 'link';
		this.title = '';
		this.url = '';
		this.path = '';
		this.pathLabel = '';
		this.parentId = parentId;
		this.description = '';
		this.extensions = '';
		this.titleTemplate = null;
		this.visibility = 'public';
		this.files = undefined;
		this.fileInputVersion++;
		this.existingFileName = null;
		this.existingFileSize = null;
		this.resetRefetchState();
		this.markPristine();
	}

	/** ピッカーで選んだ場所を入れる。 */
	choosePath(path: string, label: string): void {
		this.path = path;
		this.pathLabel = label;
	}

	/** 選んだ親グループの id。ルート直下は `null`。 */
	get parentIdValue(): number | null {
		return this.parentId === 'root' ? null : Number(this.parentId);
	}

	/** 送信・件数確認の両方で使う拡張子。archive 以外と空入力は `null`。 */
	get normalizedExtensions(): string | null {
		return this.type === 'archive' && this.extensions.trim() !== '' ? this.extensions.trim() : null;
	}

	/** JSON で送る作成・更新に共通の項目。`type` は更新で送らないので含めない。 */
	requestFields() {
		return {
			parentId: this.parentIdValue,
			title: this.title,
			url: this.type === 'link' ? this.url : null,
			path: isDirectoryContentType(this.type) ? this.path : null,
			description: this.description.trim() === '' ? null : this.description,
			visibility: this.visibility,
			extensions: this.normalizedExtensions,
			titleTemplate: this.titleTemplate
		};
	}

	/**
	 * 作成で JSON に送る項目。タイトルは group だけ送り、ほかはサーバーが自動で付ける。
	 * 種別を選び直しても、前の種別で打ったタイトルが残らないようにする。
	 */
	createRequestFields() {
		return { ...this.requestFields(), title: this.type === 'group' ? this.title : null };
	}

	/**
	 * `file` の作成・差し替えで送る multipart。ファイルを選んでいなければ `file` を含めない。
	 * 作成 (`forCreate`) ではタイトルを送らず、サーバーがファイル名から付ける。
	 */
	toFormData(forCreate = false): FormData {
		const fields = this.requestFields();
		const body = new FormData();
		if (!forCreate) body.append('title', fields.title);
		if (fields.description !== null) body.append('description', fields.description);
		body.append('visibility', fields.visibility);
		if (fields.parentId !== null) body.append('parentId', String(fields.parentId));
		const file = this.files?.[0];
		if (file) body.append('file', file);
		return body;
	}

	/** 既存のコンテンツ `id` をフォームの値で置き換える。`file` は multipart、ほかは JSON で送る。 */
	replace(id: number) {
		const params = { path: { id } };
		return this.type === 'file'
			? client.PUT('/api/v1/contents/{id}/upload', {
					params,
					// openapi-fetch は FormData をそのまま送るが、生成された型はスキーマの形のままなので
					// キャストする (実行時の挙動とは無関係)。
					body: this.toFormData() as unknown as components['schemas']['ReplaceContentUploadRequest']
				})
			: client.PUT('/api/v1/contents/{id}', { params, body: this.requestFields() });
	}

	/**
	 * 既存のコンテンツを読み込む。`canSeeFullPath` は、場所をフルパスで出してよい人か
	 * (→ `$lib/root-location` の `canManageRoots`)。
	 */
	loadFrom(content: Content, canSeeFullPath: boolean): void {
		this.type = content.type;
		this.title = content.title;
		this.url = content.url ?? '';
		this.path = content.path ?? '';
		this.pathLabel = contentPathLabel(content, canSeeFullPath) ?? '';
		this.parentId = content.parentId === null ? 'root' : String(content.parentId);
		this.description = content.description ?? '';
		this.extensions = content.extensions ?? '';
		this.titleTemplate = content.titleTemplate ?? null;
		this.visibility = content.visibility;
		this.files = undefined;
		this.fileInputVersion++;
		this.existingFileName = content.fileName ?? null;
		this.existingFileSize = content.fileSize ?? null;
		this.resetRefetchState();
		this.markPristine();
	}
}

/**
 * 公開範囲が保存できないときの文言。サーバーの 422 は汎用の文言にしかならないため、
 * 理由はフォーム側で示す。親子の組み合わせでは拒まない (→ docs/access.md「祖先のグループを辿る」)。
 */
export function contentFormVisibilityError(form: ContentFormState): string | null {
	// 本人のみを選んだ後に、種別をグループへ変えたとき。
	return form.type === 'group' && form.visibility === 'private'
		? m.contents_form_visibility_group_private_error()
		: null;
}

/**
 * 親グループによって、実際の見え方が設定より厳しくなるときの説明 (→ docs/ui.md「UI 全般」)。
 *
 * @param byId 管理画面の一覧を id で引く表 (親を辿るため)。
 */
export function contentFormVisibilityNote(
	form: ContentFormState,
	byId: ReadonlyMap<number, Content>
): string | null {
	const effective = contentFormEffectiveVisibility(form, byId);
	return effective === form.visibility
		? null
		: m.contents_form_visibility_inherited_help({ visibility: visibilityLabel(effective) });
}

/**
 * フォームの公開範囲に、選んだ親グループを重ねた実際の見え方 (→ docs/access.md「親が外れるときの公開範囲」)。
 * 登録前の確認で閲覧範囲を示すときも、設定ではなくこちらを使う。
 */
export function contentFormEffectiveVisibility(
	form: ContentFormState,
	byId: ReadonlyMap<number, Content>
): Visibility {
	return effectiveVisibilityUnder(form.visibility, form.parentIdValue, byId);
}
