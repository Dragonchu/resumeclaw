\NeedsTeXFormat{LaTeX2e}
\ProvidesClass{resume}[2026/03/09 Minimal resume template inspired by billryan/resume]
\LoadClass[11pt]{article}

\RequirePackage[
  a4paper,
  left=0.85in,
  right=0.85in,
  top=0.75in,
  bottom=0.65in,
  nohead
]{geometry}
\RequirePackage{enumitem}
\RequirePackage{fontspec}
\RequirePackage{hyperref}
\RequirePackage{titlesec}
\RequirePackage{xifthen}

\hypersetup{hidelinks}
\urlstyle{same}
\defaultfontfeatures{Ligatures=TeX}
\IfFontExistsTF{Latin Modern Roman}{
  \setmainfont{Latin Modern Roman}
}{
  \IfFontExistsTF{TeX Gyre Termes}{
    \setmainfont{TeX Gyre Termes}
  }{
    \IfFontExistsTF{TeX Gyre Pagella}{
      \setmainfont{TeX Gyre Pagella}
    }{
      \PackageWarningNoLine{resume}{No bundled serif fallback font found; using the TeX engine default Roman font}
    }
  }
}

\setlength{\parindent}{0pt}
\setlength{\parskip}{0.35em}
\setlist[itemize]{nosep, leftmargin=1.5em}
\setlist[enumerate]{nosep, leftmargin=1.5em}

\titleformat{\section}
  {\Large\bfseries\raggedright}
  {}{0em}{}
  [\titlerule]
\titlespacing*{\section}{0pt}{1.2em}{0.6em}

\titleformat{\subsection}
  {\normalsize\bfseries\raggedright}
  {}{0em}{}
\titlespacing*{\subsection}{0pt}{0.8em}{0.15em}

\newcommand{\datedsection}[2]{%
  \section[#1]{#1 \hfill #2}%
}
\newcommand{\datedsubsection}[2]{%
  \subsection[#1]{#1 \hfill #2}%
}
\newcommand{\datedline}[2]{%
  {\par #1 \hfill #2 \par}%
}

\newcommand{\name}[1]{%
  \begin{center}
    {\Huge\bfseries #1}
  \end{center}
  \vspace{0.4em}
}

\newcommand{\basicInfo}[1]{%
  \begin{center}
    \normalsize #1
  \end{center}
  \vspace{0.6em}
}

\newcommand{\role}[2]{%
  {\par \textit{#1} \hfill #2 \par}
  \vspace{0.2em}
}

\newcommand{\email}[1]{\href{mailto:#1}{#1}}
\newcommand{\phone}[1]{#1}
\newcommand{\linkedin}[2][]{%
  \ifthenelse{\isempty{#1}}%
    {\href{#2}{#2}}%
    {\href{#2}{#1}}%
}
\newcommand{\github}[2][]{%
  \ifthenelse{\isempty{#1}}%
    {\href{#2}{#2}}%
    {\href{#2}{#1}}%
}
\newcommand{\homepage}[2][]{%
  \ifthenelse{\isempty{#1}}%
    {\href{#2}{#2}}%
    {\href{#2}{#1}}%
}
