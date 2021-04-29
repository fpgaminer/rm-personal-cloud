const path = require('path');
const webpack = require('webpack');

module.exports = {
	entry: {
		main: './src/index.tsx',
	},

	devtool: 'source-map',

	module: {
		rules: [
			{
				test: /\.tsx?$/,
				use: 'ts-loader',
				exclude: /node_modules/,
			},
		],
	},

	resolve: {
		extensions: [ '.tsx', '.ts', '.js' ],
		modules: [
			path.resolve('./node_modules'),
			path.resolve('./src')
		]
	},

	output: {
		filename: "[name].bundle.js",
		path: path.resolve(__dirname, 'dist'),
	},

	plugins: [

	],
};