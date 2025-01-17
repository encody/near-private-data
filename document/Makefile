all: paper

paper:
	pandoc -s -N -f markdown --template=./eisvogel.latex --csl=./acm.csl --citeproc -o paper.pdf paper.md

clean:
	rm paper.pdf

.PHONY: all clean paper
