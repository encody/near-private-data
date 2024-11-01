all: paper

paper:
	@pandoc -s -F pandoc-crossref --natbib meta.yaml --template=mytemplate.tex -N \
	 -f markdown -t latex+raw_tex+tex_math_dollars+citations -o main.tex main.md
	@pdflatex main.tex &> /dev/null
	@bibtex main &> /dev/null
	@pdflatex main.tex &> /dev/null
	@pdflatex main.tex &> /dev/null

clean:
	rm main.aux main.tex main.log main.bbl main.blg main.out

.PHONY: all clean paper
