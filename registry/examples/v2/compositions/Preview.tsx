import {AbsoluteFill, Composition} from "remotion";

export function PreviewRoot() {
  return <Composition id="Preview" component={Preview} durationInFrames={30} fps={30} width={640} height={360} />;
}

function Preview() {
  return <AbsoluteFill style={{backgroundColor: "#ff7f50"}}>Example</AbsoluteFill>;
}
