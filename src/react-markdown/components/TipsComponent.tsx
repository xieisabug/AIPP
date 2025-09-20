import React from "react";
import { Info } from "lucide-react";

interface TipsComponentProps {
    text: string;
}

const TipsComponent: React.FC<TipsComponentProps> = ({ text }) => {
    return (
        <div className="border border-gray-300 p-2.5 rounded bg-gray-50">
            <Info className="inline-block mr-1" size={16} /> {text}
        </div>
    );
};

export default TipsComponent;
