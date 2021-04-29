import * as React from 'react';
let pdfjs = require("pdfjs-dist/webpack");
import { v4 as uuidv4 } from 'uuid';
import * as JSZip from 'jszip';

interface State {
	uploading: boolean;
	device_code: string;
}

export class App extends React.Component<{}, State> {
	private fileInput = React.createRef<HTMLInputElement>();

	state = {
		uploading: false,
		device_code: "",
	};

	onSubmit = async (event: React.FormEvent) => {
		event.preventDefault();

		if (this.fileInput.current === null || this.fileInput.current.files === null || this.fileInput.current.files.length < 1) {
			return;
		}

		console.log(this.fileInput.current.files[0]);

		const file = this.fileInput.current.files[0];
		const buffer = await file.arrayBuffer();
		const filename = file.name.replace(/\.[^/.]+$/, ""); // Remove extension
		let file_type, num_pages;

		if (file.type.startsWith("application/pdf")) {
			// Parse the PDF and determine the number of pages
			const doc = await pdfjs.getDocument(new Uint8Array(buffer)).promise;
			num_pages = doc.numPages;
			file_type = "pdf";
		}
		else if (file.type.startsWith("application/epub")) {
			// The reMarkable app doesn't parse epubs, so we won't either.
			num_pages = 1;
			file_type = "epub";
		}
		else {
			alert("Sorry, we can only handle PDF or EPUB files right now.");
			return;
		}

		// Build archive
		const id = uuidv4();
		const zip = new JSZip();

		zip.file(`${id}.content`, buildContent(file_type, num_pages));
		zip.file(`${id}.pagedata`, "Blank\n".repeat(num_pages));
		zip.file(`${id}.${file_type}`, buffer);

		const zipped_file = await zip.generateAsync({type: "blob"});

		// User token
		const user_token = await this.fetchUserToken();

		// Request upload
		const response = await fetch("/document-storage/json/2/upload/request", {
			method: 'put',
			headers: new Headers({
				'Authorization': `Bearer ${user_token}`,
				'Content-Type': 'application/json'
			}),
			body: JSON.stringify([{
				ID: id,
				Version: 1,
			}]),
		});
		const response_json = await response.json();
		const upload_url = new URL(response_json[0].BlobURLPut);

		// Rewrite host, since the API returns a URL compatible with the tablet's proxy.
		upload_url.hostname = window.location.hostname;
		upload_url.port = window.location.port;

		// Upload
		await fetch(upload_url.href, {
			method: 'put',
			body: zipped_file,
		});

		// Finish
		await fetch("/document-storage/json/2/upload/update-status", {
			method: 'put',
			headers: new Headers({
				'Authorization': `Bearer ${user_token}`,
				'Content-Type': 'application/json'
			}),
			body: JSON.stringify([{
				ID: id,
				Version: 1,
				ModifiedClient: new Date().toISOString(),
				Type: "DocumentType",
				VissibleName: filename,
				CurrentPage: 0,
				Bookmarked: false,
				Parent: "",
			}]),
		});

		alert("Upload successful");
	}

	async fetchUserToken() {
		// Use our admin token to fetch a user token, so we can access the normal api.
		const response = await fetch('/admin/new_user_token', {
			method: 'post',
			headers: new Headers({
				'Authorization': 'Bearer ' + window.location.hash.substring(1),
			}),
		});

		return await response.text();
	}

	async fetchNewDeviceCode() {
		const response = await fetch('/admin/new_device_code', {
			method: 'post',
			headers: new Headers({
				'Authorization': 'Bearer ' + window.location.hash.substring(1),
			}),
		});

		if (!response.ok) {
			throw Error(response.statusText);
		}

		const response_json = await response.json();

		return response_json['code'];
	}

	async componentDidMount() {
		try {
			const device_code = await this.fetchNewDeviceCode();

			this.setState({device_code: device_code});
		} catch (error) {
			alert("Error: " + error);
		}
	}

	render() {
		return <div className="app">
			New Device Code: <div className='device_code'>{this.state.device_code}</div>
			<br/>
			<form onSubmit={ async (event) => { this.setState({uploading: true}); await this.onSubmit(event); this.setState({uploading: false}); }}>
				<label>Upload file: <input type="file" ref={this.fileInput} accept=".pdf,.epub" /></label>
				<br/>
				<button disabled={this.state.uploading} type="submit">Upload</button>
			</form>
		</div>
	}
}


function buildContent(file_type: string, num_pages: number): string {
	const pages = Array.from({length: num_pages}, () => uuidv4());

	return JSON.stringify({
		"coverPageNumber": 0,
		"dummyDocument": false,
		"extraMetadata": {
		},
		"fileType": file_type,
		"fontName": "",
		"lineHeight": -1,
		"margins": 100,
		"orientation": "portrait",
		"pageCount": num_pages,
		"pages": pages,
		"textAlignment": "left",
		"textScale": 1,
		"transform": {
			"m11": 1,
			"m12": 0,
			"m13": 0,
			"m21": 0,
			"m22": 1,
			"m23": 0,
			"m31": 0,
			"m32": 0,
			"m33": 1
		}
	}, null, 4);
}