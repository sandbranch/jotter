(* jotter, Braille mode: plots as SVG with their numbers as text. The
   plot area is a picture; tick numbers (FindDivisions), the PlotLabel and
   the AxesLabel are <text>. The Wolfram Language for Jupyter kernel sends
   JotterBraille`textSVG[result] as image/svg+xml when
   JotterBraille`$TextSVG is True and the result is a Graphics; that hook is
   a local change to its RequestHandlers.wl. *)
JotterBraille`$TextSVG = True;
JotterBraille`textSVG[g_Graphics] := Quiet @ Check[Module[
  {pr, ar, inner, img, w, h, x0, x1, y0, y1, xt, yt, fmt, esc, lab, title, axl, xl, yl,
   l, t, b, r = 12, px, py, txt, png},
  pr = PlotRange /. AbsoluteOptions[g, PlotRange];
  If[! MatchQ[pr, {{_?NumericQ, _?NumericQ}, {_?NumericQ, _?NumericQ}}], Return[$Failed, Module]];
  {{x0, x1}, {y0, y1}} = N[pr];
  ar = AspectRatio /. AbsoluteOptions[g, AspectRatio];
  inner = Show[g, Ticks -> None, AxesLabel -> None, PlotLabel -> None, Frame -> False,
    FrameLabel -> None, FrameTicks -> None, ImagePadding -> 0, PlotRange -> pr,
    PlotRangePadding -> None, PlotRangeClipping -> True, ImageSize -> 480,
    AspectRatio -> If[NumericQ[ar], ar, Automatic], Background -> White];
  img = Rasterize[inner, ImageResolution -> 72];
  {w, h} = ImageDimensions[img];
  fmt[v_] := If[Abs[v - Round[v]] < 10^-9 Max[1, Abs[v]], ToString[Round[v]],
    TextString[NumberForm[N[v], 4]]];
  esc[s_String] := StringReplace[s, {"&" -> "&amp;", "<" -> "&lt;", ">" -> "&gt;"}];
  lab[x_] := If[x === None || x === "" , None, esc[TextString[x]]];
  xt = Select[N[FindDivisions[{x0, x1}, 6]], x0 - 10^-9 <= # <= x1 + 10^-9 &];
  yt = Select[N[FindDivisions[{y0, y1}, 5]], y0 - 10^-9 <= # <= y1 + 10^-9 &];
  title = lab[PlotLabel /. Options[g, PlotLabel] /. PlotLabel -> None];
  axl = AxesLabel /. Options[g, AxesLabel] /. AxesLabel -> None;
  {xl, yl} = Which[ListQ[axl] && Length[axl] == 2, lab /@ axl, axl =!= None, {lab[axl], None},
    True, {None, None}];
  l = 16 + 8 Max[StringLength /@ (fmt /@ yt), 1] + If[yl =!= None, 18, 0];
  t = If[title =!= None, 30, 12];
  b = 26 + If[xl =!= None, 20, 0];
  px[x_] := l + (x - x0)/(x1 - x0) w;
  py[y_] := t + (y1 - y)/(y1 - y0) h;
  txt[x_, y_, s_, anchor_, extra_ : ""] := StringJoin["<text x=\"", ToString[Round[x]],
    "\" y=\"", ToString[Round[y]], "\" text-anchor=\"", anchor, "\"", extra,
    " font-family=\"sans-serif\" font-size=\"12\">", s, "</text>\n"];
  png = BaseEncode[ExportByteArray[img, "PNG"]];
  StringJoin[
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"", ToString[l + w + r], "\" height=\"",
    ToString[t + h + b], "\">\n",
    "<rect x=\"0\" y=\"0\" width=\"", ToString[l + w + r], "\" height=\"", ToString[t + h + b],
    "\" fill=\"white\"/>\n",
    "<image x=\"", ToString[l], "\" y=\"", ToString[t], "\" width=\"", ToString[w], "\" height=\"",
    ToString[h], "\" href=\"data:image/png;base64,", png, "\"/>\n",
    "<rect x=\"", ToString[l], "\" y=\"", ToString[t], "\" width=\"", ToString[w], "\" height=\"",
    ToString[h], "\" fill=\"none\" stroke=\"black\"/>\n",
    txt[px[#], t + h + 16, fmt[#], "middle"] & /@ xt,
    txt[l - 6, py[#] + 4, fmt[#], "end"] & /@ yt,
    If[title =!= None, txt[l + w/2, 18, title, "middle"], ""],
    If[xl =!= None, txt[l + w/2, t + h + 38, xl, "middle"], ""],
    If[yl =!= None, txt[14, t + h/2, yl, "middle",
      " transform=\"rotate(-90 14 " <> ToString[Round[t + h/2]] <> ")\""], ""],
    "</svg>\n"]
], $Failed];
