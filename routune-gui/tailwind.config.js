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
        theme: {
          bg: "#0F0E17",
          surface: "#171528",
          surfaceHover: "#201D36",
          surfaceActive: "#282444",
          border: "#2B2942",
          borderHover: "#3F3B5F",
          text: "#ECEAFB",
          textMuted: "#A5A0C8",
          textDim: "#7A759E",
          primary: "#7C6AF2",
          primaryHover: "#8E7EF7",
          primaryDark: "#6856E3",
          accent: "#C4B5FD",
        },
        brand: {
          DEFAULT: "#7C6AF2",
          primary: "#7C6AF2",
          accent: "#C4B5FD",
          bg: "#0F0E17",
          surface: "#171528",
          border: "#2B2942",
          text: "#ECEAFB",
          orange: "#7C6AF2",
          orangeLight: "#8E7EF7",
          orangeDark: "#6856E3",
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
