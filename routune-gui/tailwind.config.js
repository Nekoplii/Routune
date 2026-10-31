/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      borderWidth: {
        DEFAULT: '2px',
        '0': '0px',
        '2': '2px',
        '4': '4px',
        '8': '8px',
      },
      colors: {
        brand: {
          orange: "#ff5511",
          orangeLight: "#ff6622",
          orangeDark: "#e04400",
        },
      },
      fontFamily: {
        sans: ['Inter', '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Roboto', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'monospace'],
      },
    },
  },
  plugins: [],
}
