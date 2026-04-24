/** @type {import('tailwindcss').Config} */
export default {
    content: [
        "./index.html",
        "./src/**/*.{js,ts,jsx,tsx}",
    ],
    theme: {
        extend: {
            colors: {
                twitch: {
                    DEFAULT: '#9146FF',
                    dark: '#772CE8',
                    light: '#A970FF'
                },
                dark: {
                    bg: '#0E0E10',
                    surface: '#18181B',
                    surface2: '#1F1F23',
                    text: '#EFEFF1'
                }
            }
        },
    },
    plugins: [],
}
