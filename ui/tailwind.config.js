/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/**/*.{rs,tsx,html}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Chain colors
        bitcoin: {
          DEFAULT: "#F7931A",
          bg: "rgba(247, 147, 26, 0.2)",
          border: "rgba(247, 147, 26, 0.3)",
        },
        ethereum: {
          DEFAULT: "#627EEA",
          bg: "rgba(98, 126, 234, 0.2)",
          border: "rgba(98, 126, 234, 0.3)",
        },
        sui: {
          DEFAULT: "#06BDFF",
          bg: "rgba(6, 189, 255, 0.2)",
          border: "rgba(6, 189, 255, 0.3)",
        },
        aptos: {
          DEFAULT: "#2DD8A3",
          bg: "rgba(45, 216, 163, 0.2)",
          border: "rgba(45, 216, 163, 0.3)",
        },
        solana: {
          DEFAULT: "#9945FF",
          bg: "rgba(153, 69, 255, 0.2)",
          border: "rgba(153, 69, 255, 0.3)",
        },
        // Status colors
        success: {
          DEFAULT: "#22C55E",
          bg: "rgba(34, 197, 94, 0.2)",
        },
        warning: {
          DEFAULT: "#EAB308",
          bg: "rgba(234, 179, 8, 0.2)",
        },
        error: {
          DEFAULT: "#EF4444",
          bg: "rgba(239, 68, 68, 0.2)",
        },
      },
      fontFamily: {
        mono: ["JetBrains Mono", "Fira Code", "monospace"],
      },
      animation: {
        "pulse-slow": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
      },
    },
  },
  plugins: [],
};
