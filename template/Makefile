APP := main

LATEX := pdflatex
BIBTEX := bibtex
LATEX_FLAGS := -shell-escape -halt-on-error -synctex=1

.PHONY: all clean distclean

.SUFFIX: .tex .pdf

%.pdf: %.tex
	$(LATEX) $(LATEX_FLAGS) $<
	$(BIBTEX) $(APP)
	$(LATEX) $(LATEX_FLAGS) $<
	$(LATEX) $(LATEX_FLAGS) $<

all: $(APP).pdf

distclean:
	$(RM) $(APP).pdf

clean:
	$(RM) *~ $(APP).dvi $(APP).aux $(APP).toc $(APP).out \
	$(APP).log $(APP).pdf $(APP).ps $(APP).blg
