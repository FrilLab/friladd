import './photo-stack.css';

export function PhotoStack({ photos }: { photos: string[] }) {
  return (
    <div className="photo-stack">
      {photos.slice(0, 3).map((photo, index) => (
        <img key={`${photo}-${index}`} src={photo} alt="" />
      ))}
    </div>
  );
}
